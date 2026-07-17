use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension, Row};

use super::database::{ensure_supported_schema, open_local_database, run_migration};
use super::domain::{
    AdapterKind, EventEnvelope, EventPhase, ReplayResult, RunSnapshot, RunStatus, StartRunRequest,
    TerminalState,
};
use super::error::{KruonError, KruonResult};

pub struct EventStore {
    connection: Mutex<Connection>,
}

impl EventStore {
    pub fn open(path: impl AsRef<Path>) -> KruonResult<Self> {
        let connection = open_local_database(path)?;
        ensure_supported_schema(&connection)?;
        Self::configure(&connection, true)?;
        let store = Self {
            connection: Mutex::new(connection),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> KruonResult<Self> {
        let connection = Connection::open_in_memory()?;
        Self::configure(&connection, false)?;
        let store = Self {
            connection: Mutex::new(connection),
        };
        store.migrate()?;
        Ok(store)
    }

    fn configure(connection: &Connection, wal: bool) -> KruonResult<()> {
        if wal {
            connection.pragma_update(None, "journal_mode", "WAL")?;
        }
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(Duration::from_secs(5))?;
        Ok(())
    }

    fn migrate(&self) -> KruonResult<()> {
        let mut connection = self.connection.lock().expect("event store mutex poisoned");
        run_migration(&mut connection, |transaction| {
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS runs (
                run_id TEXT PRIMARY KEY,
                adapter TEXT NOT NULL,
                workspace_root TEXT NOT NULL,
                working_directory TEXT NOT NULL,
                policy_id TEXT,
                prompt_hash TEXT NOT NULL,
                launch_fingerprint TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL,
                terminal_state TEXT,
                last_sequence INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                pid INTEGER,
                pgid INTEGER
            );
            CREATE TABLE IF NOT EXISTS events (
                event_id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL,
                sequence INTEGER NOT NULL,
                schema_version INTEGER NOT NULL,
                event_type TEXT NOT NULL,
                phase TEXT NOT NULL,
                occurred_at TEXT NOT NULL,
                terminal_state TEXT,
                envelope_json TEXT NOT NULL,
                UNIQUE(run_id, sequence),
                FOREIGN KEY(run_id) REFERENCES runs(run_id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS events_run_sequence
                ON events(run_id, sequence);
            INSERT OR IGNORE INTO schema_migrations(version, applied_at)
                VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));",
            )?;
            let has_launch_fingerprint = {
                let mut columns = transaction.prepare("PRAGMA table_info(runs)")?;
                let rows = columns.query_map([], |row| row.get::<_, String>(1))?;
                rows.collect::<Result<Vec<_>, _>>()?
                    .iter()
                    .any(|name| name == "launch_fingerprint")
            };
            if !has_launch_fingerprint {
                transaction.execute(
                    "ALTER TABLE runs ADD COLUMN launch_fingerprint TEXT NOT NULL DEFAULT ''",
                    [],
                )?;
            }
            transaction.execute(
                "INSERT OR IGNORE INTO schema_migrations(version, applied_at)
                 VALUES (2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                [],
            )?;
            Ok(())
        })
    }

    pub fn create_run(
        &self,
        run_id: &str,
        request: &StartRunRequest,
        workspace_root: &Path,
        working_directory: &Path,
        launch_fingerprint: &str,
    ) -> KruonResult<RunSnapshot> {
        let now = chrono::Utc::now().to_rfc3339();
        let snapshot = RunSnapshot {
            run_id: run_id.to_owned(),
            adapter: request.adapter,
            workspace_root: workspace_root.to_string_lossy().into_owned(),
            working_directory: working_directory.to_string_lossy().into_owned(),
            policy_id: request.policy_id.clone(),
            status: RunStatus::Pending,
            terminal_state: None,
            created_at: now.clone(),
            updated_at: now,
            last_sequence: 0,
            prompt_hash: request.prompt_hash(),
            launch_fingerprint: launch_fingerprint.to_owned(),
            pid: None,
            pgid: None,
        };
        let connection = self.connection.lock().expect("event store mutex poisoned");
        connection.execute(
            "INSERT INTO runs(
                run_id, adapter, workspace_root, working_directory, policy_id, prompt_hash,
                launch_fingerprint, status, terminal_state, last_sequence, created_at, updated_at, pid, pgid
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, 0, ?9, ?10, NULL, NULL)",
            params![
                snapshot.run_id,
                encode(&snapshot.adapter)?,
                snapshot.workspace_root,
                snapshot.working_directory,
                snapshot.policy_id,
                snapshot.prompt_hash,
                snapshot.launch_fingerprint,
                encode(&snapshot.status)?,
                snapshot.created_at,
                snapshot.updated_at,
            ],
        )?;
        Ok(snapshot)
    }

    pub fn update_process(&self, run_id: &str, pid: u32, pgid: i32) -> KruonResult<()> {
        let connection = self.connection.lock().expect("event store mutex poisoned");
        let changed = connection.execute(
            "UPDATE runs SET pid = ?1, pgid = ?2, updated_at = ?3 WHERE run_id = ?4",
            params![pid, pgid, chrono::Utc::now().to_rfc3339(), run_id],
        )?;
        if changed == 0 {
            return Err(KruonError::NotFound(run_id.to_owned()));
        }
        Ok(())
    }

    pub fn append_event(&self, event: &EventEnvelope) -> KruonResult<bool> {
        let envelope_json = serde_json::to_string(event)?;
        let mut connection = self.connection.lock().expect("event store mutex poisoned");
        let transaction = connection.transaction()?;

        let existing: Option<String> = transaction
            .query_row(
                "SELECT envelope_json FROM events WHERE event_id = ?1",
                params![event.event_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing_json) = existing {
            if existing_json == envelope_json {
                transaction.rollback()?;
                return Ok(false);
            }
            return Err(KruonError::Conflict(format!(
                "event_id {} already exists with different content",
                event.event_id
            )));
        }

        let projection: Option<(i64, Option<String>)> = transaction
            .query_row(
                "SELECT last_sequence, terminal_state FROM runs WHERE run_id = ?1",
                params![event.run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (last_sequence, current_terminal) =
            projection.ok_or_else(|| KruonError::NotFound(event.run_id.clone()))?;
        if current_terminal.is_some() {
            return Err(KruonError::Conflict(format!(
                "run {} already has a terminal event",
                event.run_id
            )));
        }
        let event_sequence = sql_integer(event.sequence)?;
        let expected = last_sequence + 1;
        if event_sequence != expected {
            return Err(KruonError::Conflict(format!(
                "run {} expected sequence {}, got {}",
                event.run_id, expected, event.sequence
            )));
        }

        let status = projected_status(event);
        let terminal_json = event
            .terminal_state
            .map(|state| encode(&state))
            .transpose()?;
        transaction.execute(
            "INSERT INTO events(
                event_id, run_id, sequence, schema_version, event_type, phase,
                occurred_at, terminal_state, envelope_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                event.event_id,
                event.run_id,
                event_sequence,
                event.schema_version,
                event.event_type,
                encode(&event.phase)?,
                event.occurred_at,
                terminal_json,
                envelope_json,
            ],
        )?;
        transaction.execute(
            "UPDATE runs
             SET status = ?1, terminal_state = ?2, last_sequence = ?3, updated_at = ?4
             WHERE run_id = ?5",
            params![
                encode(&status)?,
                event
                    .terminal_state
                    .map(|state| encode(&state))
                    .transpose()?,
                event_sequence,
                event.occurred_at,
                event.run_id,
            ],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn get_run(&self, run_id: &str) -> KruonResult<RunSnapshot> {
        let connection = self.connection.lock().expect("event store mutex poisoned");
        connection
            .query_row(
                "SELECT run_id, adapter, workspace_root, working_directory, policy_id,
                        status, terminal_state, created_at, updated_at, last_sequence,
                        prompt_hash, launch_fingerprint, pid, pgid
                 FROM runs WHERE run_id = ?1",
                params![run_id],
                read_run,
            )
            .optional()?
            .ok_or_else(|| KruonError::NotFound(run_id.to_owned()))
    }

    pub fn list_runs(&self) -> KruonResult<Vec<RunSnapshot>> {
        let connection = self.connection.lock().expect("event store mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT run_id, adapter, workspace_root, working_directory, policy_id,
                    status, terminal_state, created_at, updated_at, last_sequence,
                    prompt_hash, launch_fingerprint, pid, pgid
             FROM runs ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map([], read_run)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn schema_versions(&self) -> KruonResult<Vec<i64>> {
        let connection = self.connection.lock().expect("event store mutex poisoned");
        let mut statement =
            connection.prepare("SELECT version FROM schema_migrations ORDER BY version")?;
        let rows = statement.query_map([], |row| row.get::<_, i64>(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn active_run_count(&self) -> KruonResult<usize> {
        let connection = self.connection.lock().expect("event store mutex poisoned");
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM runs WHERE terminal_state IS NULL",
            [],
            |row| row.get(0),
        )?;
        usize::try_from(count)
            .map_err(|_| KruonError::Store("active run count exceeds platform size".into()))
    }

    pub fn list_events(
        &self,
        run_id: &str,
        after_sequence: u64,
    ) -> KruonResult<Vec<EventEnvelope>> {
        let connection = self.connection.lock().expect("event store mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT envelope_json FROM events
             WHERE run_id = ?1 AND sequence > ?2 ORDER BY sequence ASC",
        )?;
        let rows = statement.query_map(params![run_id, sql_integer(after_sequence)?], |row| {
            row.get::<_, String>(0)
        })?;
        let mut events = Vec::new();
        for row in rows {
            events.push(serde_json::from_str(&row?)?);
        }
        Ok(events)
    }

    pub fn replay_run(&self, run_id: &str) -> KruonResult<ReplayResult> {
        let run = self.get_run(run_id)?;
        let events = self.list_events(run_id, 0)?;
        for (index, event) in events.iter().enumerate() {
            let expected = index as u64 + 1;
            if event.sequence != expected {
                return Err(KruonError::Store(format!(
                    "run {run_id} has a replay gap at sequence {expected}"
                )));
            }
        }
        if events.len() as u64 != run.last_sequence {
            return Err(KruonError::Store(format!(
                "run {run_id} projection/event count mismatch"
            )));
        }
        Ok(ReplayResult { run, events })
    }

    pub fn recover_interrupted_runs(&self) -> KruonResult<Vec<RunSnapshot>> {
        let run_ids = {
            let connection = self.connection.lock().expect("event store mutex poisoned");
            let mut statement = connection.prepare(
                "SELECT run_id FROM runs WHERE terminal_state IS NULL ORDER BY created_at ASC",
            )?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let mut recovered = Vec::new();
        for run_id in run_ids {
            let snapshot = self.get_run(&run_id)?;
            let event = EventEnvelope::new(
                &run_id,
                snapshot.last_sequence + 1,
                "run.recovery_uncertain",
                EventPhase::Uncertain,
                Some(TerminalState::Unknown),
                serde_json::json!({
                    "reason": "application restarted without an in-memory process handle",
                    "previous_status": snapshot.status,
                    "previous_pid": snapshot.pid,
                    "previous_pgid": snapshot.pgid,
                }),
            );
            self.append_event(&event)?;
            recovered.push(self.get_run(&run_id)?);
        }
        Ok(recovered)
    }
}

fn encode<T: serde::Serialize>(value: &T) -> KruonResult<String> {
    Ok(serde_json::to_string(value)?)
}

fn decode<T: serde::de::DeserializeOwned>(value: String, column: usize) -> rusqlite::Result<T> {
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn read_run(row: &Row<'_>) -> rusqlite::Result<RunSnapshot> {
    let adapter: String = row.get(1)?;
    let status: String = row.get(5)?;
    let terminal: Option<String> = row.get(6)?;
    Ok(RunSnapshot {
        run_id: row.get(0)?,
        adapter: decode::<AdapterKind>(adapter, 1)?,
        workspace_root: row.get(2)?,
        working_directory: row.get(3)?,
        policy_id: row.get(4)?,
        status: decode::<RunStatus>(status, 5)?,
        terminal_state: terminal
            .map(|value| decode::<TerminalState>(value, 6))
            .transpose()?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        last_sequence: u64::try_from(row.get::<_, i64>(9)?).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                9,
                rusqlite::types::Type::Integer,
                Box::new(error),
            )
        })?,
        prompt_hash: row.get(10)?,
        launch_fingerprint: row.get(11)?,
        pid: row.get(12)?,
        pgid: row.get(13)?,
    })
}

fn projected_status(event: &EventEnvelope) -> RunStatus {
    if let Some(terminal) = event.terminal_state {
        return match terminal {
            TerminalState::Completed => RunStatus::Completed,
            TerminalState::Failed => RunStatus::Failed,
            TerminalState::Cancelled => RunStatus::Cancelled,
            TerminalState::ForcedStopRequired => RunStatus::ForcedStopRequired,
            TerminalState::Unknown => RunStatus::Uncertain,
        };
    }
    if event.event_type == "run.forced_stop_required" {
        return RunStatus::ForcedStopRequired;
    }
    match event.phase {
        EventPhase::Setup => RunStatus::Pending,
        EventPhase::Planning => RunStatus::Planning,
        EventPhase::WaitingApproval => RunStatus::WaitingApproval,
        EventPhase::Cancelling => RunStatus::Cancelling,
        EventPhase::Uncertain => RunStatus::Uncertain,
        _ => RunStatus::Running,
    }
}

fn sql_integer(value: u64) -> KruonResult<i64> {
    i64::try_from(value)
        .map_err(|_| KruonError::InvalidArgument(format!("integer {value} exceeds SQLite range")))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use super::*;

    fn request(root: &Path) -> StartRunRequest {
        StartRunRequest {
            adapter: AdapterKind::Codex,
            workspace_root: root.to_string_lossy().into_owned(),
            working_directory: root.to_string_lossy().into_owned(),
            prompt: "prompt that must only be stored as a hash".into(),
            timeout_ms: Some(1_000),
            policy_id: Some("test-policy".into()),
        }
    }

    fn store_with_run(run_id: &str) -> (Arc<EventStore>, tempfile::TempDir) {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(EventStore::open_in_memory().unwrap());
        store
            .create_run(
                run_id,
                &request(root.path()),
                root.path(),
                root.path(),
                "test-launch-fingerprint",
            )
            .unwrap();
        (store, root)
    }

    fn event(run_id: &str, sequence: u64, value: i32) -> EventEnvelope {
        let mut event = EventEnvelope::new(
            run_id,
            sequence,
            "test.event",
            EventPhase::Running,
            None,
            serde_json::json!({"value": value}),
        );
        event.event_id = format!("event-{run_id}-{sequence}-{value}");
        event.occurred_at = format!("2026-07-14T00:00:{sequence:02}Z");
        event
    }

    #[test]
    fn appends_contiguous_events_and_replays_projection() {
        let (store, _root) = store_with_run("contiguous");
        assert!(store.append_event(&event("contiguous", 1, 1)).unwrap());
        assert!(store.append_event(&event("contiguous", 2, 2)).unwrap());
        let terminal = EventEnvelope::new(
            "contiguous",
            3,
            "run.terminal",
            EventPhase::Terminal,
            Some(TerminalState::Completed),
            serde_json::json!({}),
        );
        store.append_event(&terminal).unwrap();
        let replay = store.replay_run("contiguous").unwrap();
        assert_eq!(replay.events.len(), 3);
        assert_eq!(replay.run.status, RunStatus::Completed);
        assert_eq!(replay.run.terminal_state, Some(TerminalState::Completed));
    }

    #[test]
    fn exact_duplicate_is_idempotent_but_changed_envelope_conflicts() {
        let (store, _root) = store_with_run("duplicate");
        let original = event("duplicate", 1, 1);
        assert!(store.append_event(&original).unwrap());
        assert!(!store.append_event(&original).unwrap());

        let mut changed = original.clone();
        changed.payload = serde_json::json!({"value": 2});
        assert!(matches!(
            store.append_event(&changed),
            Err(KruonError::Conflict(_))
        ));
        assert_eq!(store.list_events("duplicate", 0).unwrap().len(), 1);
    }

    #[test]
    fn rejects_gaps_and_events_after_terminal_without_partial_updates() {
        let (store, _root) = store_with_run("ordering");
        assert!(matches!(
            store.append_event(&event("ordering", 2, 2)),
            Err(KruonError::Conflict(_))
        ));
        assert_eq!(store.get_run("ordering").unwrap().last_sequence, 0);

        let terminal = EventEnvelope::new(
            "ordering",
            1,
            "run.terminal",
            EventPhase::Terminal,
            Some(TerminalState::Failed),
            serde_json::json!({}),
        );
        store.append_event(&terminal).unwrap();
        assert!(matches!(
            store.append_event(&event("ordering", 2, 2)),
            Err(KruonError::Conflict(_))
        ));
        assert_eq!(store.list_events("ordering", 0).unwrap().len(), 1);
    }

    #[test]
    fn restart_recovery_marks_nonterminal_runs_uncertain() {
        let (store, _root) = store_with_run("recovery");
        store.append_event(&event("recovery", 1, 1)).unwrap();
        let recovered = store.recover_interrupted_runs().unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].status, RunStatus::Uncertain);
        assert_eq!(recovered[0].terminal_state, Some(TerminalState::Unknown));
        assert_eq!(store.replay_run("recovery").unwrap().events.len(), 2);
        assert!(store.recover_interrupted_runs().unwrap().is_empty());
    }

    #[test]
    fn concurrent_same_sequence_allows_only_one_writer() {
        let (store, _root) = store_with_run("concurrent");
        let barrier = Arc::new(Barrier::new(2));
        let mut workers = Vec::new();
        for value in [1, 2] {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                store.append_event(&event("concurrent", 1, value))
            }));
        }
        let successes = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(Result::is_ok)
            .count();
        assert_eq!(successes, 1);
        assert_eq!(store.list_events("concurrent", 0).unwrap().len(), 1);
    }

    #[test]
    fn stores_prompt_hash_not_prompt() {
        let (store, _root) = store_with_run("hash");
        let snapshot = store.get_run("hash").unwrap();
        assert_eq!(snapshot.prompt_hash.len(), 64);
        assert!(!snapshot.prompt_hash.contains("prompt"));
    }

    #[test]
    fn legacy_schema_migrates_transactionally_without_losing_runs() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("legacy.sqlite3");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at TEXT NOT NULL
                );
                INSERT INTO schema_migrations(version, applied_at) VALUES (1, 'legacy');
                CREATE TABLE runs (
                    run_id TEXT PRIMARY KEY,
                    adapter TEXT NOT NULL,
                    workspace_root TEXT NOT NULL,
                    working_directory TEXT NOT NULL,
                    policy_id TEXT,
                    prompt_hash TEXT NOT NULL,
                    status TEXT NOT NULL,
                    terminal_state TEXT,
                    last_sequence INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    pid INTEGER,
                    pgid INTEGER
                );
                CREATE TABLE events (
                    event_id TEXT PRIMARY KEY,
                    run_id TEXT NOT NULL,
                    sequence INTEGER NOT NULL,
                    schema_version INTEGER NOT NULL,
                    event_type TEXT NOT NULL,
                    phase TEXT NOT NULL,
                    occurred_at TEXT NOT NULL,
                    terminal_state TEXT,
                    envelope_json TEXT NOT NULL,
                    UNIQUE(run_id, sequence)
                );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO runs(
                    run_id, adapter, workspace_root, working_directory, policy_id, prompt_hash,
                    status, terminal_state, last_sequence, created_at, updated_at, pid, pgid
                 ) VALUES (?1, ?2, ?3, ?3, NULL, ?4, ?5, NULL, 0, ?6, ?6, NULL, NULL)",
                params![
                    "legacy-run",
                    encode(&AdapterKind::Codex).unwrap(),
                    directory.path().to_string_lossy(),
                    "legacy-prompt-hash",
                    encode(&RunStatus::Pending).unwrap(),
                    "2026-07-14T00:00:00Z",
                ],
            )
            .unwrap();
        drop(connection);

        let store = EventStore::open(&database).unwrap();
        let legacy = store.get_run("legacy-run").unwrap();
        assert_eq!(legacy.launch_fingerprint, "");
        let versions = {
            let connection = store.connection.lock().unwrap();
            let mut statement = connection
                .prepare("SELECT version FROM schema_migrations ORDER BY version")
                .unwrap();
            statement
                .query_map([], |row| row.get::<_, i64>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert_eq!(versions, vec![1, 2]);
    }

    #[test]
    fn sqlite_full_rolls_back_event_and_run_projection_together() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("full.sqlite3");
        let store = EventStore::open(&database).unwrap();
        store
            .create_run(
                "disk-full",
                &request(directory.path()),
                directory.path(),
                directory.path(),
                "fault-injection",
            )
            .unwrap();
        {
            let connection = store.connection.lock().unwrap();
            let page_count: i64 = connection
                .pragma_query_value(None, "page_count", |row| row.get(0))
                .unwrap();
            connection
                .pragma_update(None, "max_page_count", page_count)
                .unwrap();
        }

        let mut oversized = event("disk-full", 1, 1);
        oversized.payload = serde_json::json!({"blob": "x".repeat(8 * 1024 * 1024)});
        assert!(matches!(
            store.append_event(&oversized),
            Err(KruonError::Store(_))
        ));
        assert_eq!(store.get_run("disk-full").unwrap().last_sequence, 0);
        assert!(store.list_events("disk-full", 0).unwrap().is_empty());
    }
}
