use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::adapter_host::{adapter_environment, resolve_adapter_program};
use super::database::{ensure_supported_schema, open_local_database, run_migration};
use super::domain::AdapterKind;
use super::error::{KruonError, KruonResult};
use super::m4::{evaluate_version_output, supported_versions, CompatibilityStatus};

pub const MAX_CONCURRENT_RUNS: usize = 2;
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCreateRequest {
    pub root: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRecord {
    pub workspace_id: String,
    pub root: String,
    pub display_name: String,
    pub trusted: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCreateRequest {
    pub workspace_id: String,
    pub title: String,
    pub goal: String,
    pub context: String,
    pub allowed_paths: Vec<String>,
    pub acceptance_criteria: String,
    pub test_plan: String,
    pub rollback_plan: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub task_id: String,
    pub workspace_id: String,
    pub title: String,
    pub goal: String,
    pub context: String,
    pub allowed_paths: Vec<String>,
    pub acceptance_criteria: String,
    pub test_plan: String,
    pub rollback_plan: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueState {
    Queued,
    Started,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueEntry {
    pub queue_id: String,
    pub task_id: String,
    pub adapter: AdapterKind,
    pub state: QueueState,
    pub run_id: Option<String>,
    pub timeout_ms: Option<u64>,
    pub failure_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueTaskRunRequest {
    pub task_id: String,
    pub adapter: AdapterKind,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Ready,
    NotFound,
    VersionCheckFailed,
    UnsupportedVersion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationStatus {
    Authenticated,
    Unauthenticated,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterConnection {
    pub adapter: AdapterKind,
    pub command: String,
    pub status: ConnectionStatus,
    pub version: Option<String>,
    pub normalized_version: Option<String>,
    pub compatibility: CompatibilityStatus,
    pub supported_versions: Vec<String>,
    pub authentication: AuthenticationStatus,
    pub approval_mode: String,
    pub capabilities: Vec<String>,
    pub detail: String,
}

pub struct M1Store {
    connection: Mutex<Connection>,
}

impl M1Store {
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
        let mut connection = self.connection.lock().expect("M1 store mutex poisoned");
        run_migration(&mut connection, |transaction| {
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS workspaces (
                    workspace_id TEXT PRIMARY KEY,
                    root TEXT NOT NULL UNIQUE,
                    display_name TEXT NOT NULL,
                    trusted INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS tasks (
                    task_id TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    title TEXT NOT NULL,
                    goal TEXT NOT NULL,
                    context TEXT NOT NULL,
                    allowed_paths_json TEXT NOT NULL,
                    acceptance_criteria TEXT NOT NULL,
                    test_plan TEXT NOT NULL,
                    rollback_plan TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS tasks_workspace_created
                    ON tasks(workspace_id, created_at DESC);
                CREATE TABLE IF NOT EXISTS run_queue (
                    queue_id TEXT PRIMARY KEY,
                    task_id TEXT NOT NULL,
                    adapter TEXT NOT NULL,
                    state TEXT NOT NULL,
                    run_id TEXT,
                    timeout_ms INTEGER,
                    failure_code TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    FOREIGN KEY(task_id) REFERENCES tasks(task_id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS run_queue_state_created
                    ON run_queue(state, created_at ASC);
                INSERT OR IGNORE INTO schema_migrations(version, applied_at)
                    VALUES (3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));",
            )?;
            Ok(())
        })
    }

    pub fn create_workspace(
        &self,
        request: WorkspaceCreateRequest,
    ) -> KruonResult<WorkspaceRecord> {
        let root = canonical_workspace_root(&request.root)?;
        let display_name = non_empty("workspace display name", &request.display_name)?;
        let now = chrono::Utc::now().to_rfc3339();
        let workspace = WorkspaceRecord {
            workspace_id: uuid::Uuid::new_v4().to_string(),
            root: root.to_string_lossy().into_owned(),
            display_name: display_name.to_owned(),
            trusted: false,
            created_at: now.clone(),
            updated_at: now,
        };
        let connection = self.connection.lock().expect("M1 store mutex poisoned");
        connection.execute(
            "INSERT INTO workspaces(workspace_id, root, display_name, trusted, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, ?4, ?5)",
            params![
                workspace.workspace_id,
                workspace.root,
                workspace.display_name,
                workspace.created_at,
                workspace.updated_at,
            ],
        )?;
        Ok(workspace)
    }

    pub fn list_workspaces(&self) -> KruonResult<Vec<WorkspaceRecord>> {
        let connection = self.connection.lock().expect("M1 store mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT workspace_id, root, display_name, trusted, created_at, updated_at
             FROM workspaces ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map([], read_workspace)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_workspace(&self, workspace_id: &str) -> KruonResult<WorkspaceRecord> {
        self.connection
            .lock()
            .expect("M1 store mutex poisoned")
            .query_row(
                "SELECT workspace_id, root, display_name, trusted, created_at, updated_at
                 FROM workspaces WHERE workspace_id = ?1",
                params![workspace_id],
                read_workspace,
            )
            .optional()?
            .ok_or_else(|| KruonError::NotFound(format!("workspace:{workspace_id}")))
    }

    pub fn set_workspace_trust(
        &self,
        workspace_id: &str,
        trusted: bool,
    ) -> KruonResult<WorkspaceRecord> {
        let changed = self
            .connection
            .lock()
            .expect("M1 store mutex poisoned")
            .execute(
                "UPDATE workspaces SET trusted = ?1, updated_at = ?2 WHERE workspace_id = ?3",
                params![
                    trusted as i64,
                    chrono::Utc::now().to_rfc3339(),
                    workspace_id
                ],
            )?;
        if changed == 0 {
            return Err(KruonError::NotFound(format!("workspace:{workspace_id}")));
        }
        self.get_workspace(workspace_id)
    }

    pub fn create_task(&self, request: TaskCreateRequest) -> KruonResult<TaskRecord> {
        self.get_workspace(&request.workspace_id)?;
        let title = non_empty("task title", &request.title)?;
        let goal = non_empty("task goal", &request.goal)?;
        let allowed_paths = normalize_scopes(&request.allowed_paths)?;
        let now = chrono::Utc::now().to_rfc3339();
        let task = TaskRecord {
            task_id: uuid::Uuid::new_v4().to_string(),
            workspace_id: request.workspace_id,
            title: title.to_owned(),
            goal: goal.to_owned(),
            context: request.context.trim().to_owned(),
            allowed_paths,
            acceptance_criteria: request.acceptance_criteria.trim().to_owned(),
            test_plan: request.test_plan.trim().to_owned(),
            rollback_plan: request.rollback_plan.trim().to_owned(),
            created_at: now.clone(),
            updated_at: now,
        };
        let connection = self.connection.lock().expect("M1 store mutex poisoned");
        connection.execute(
            "INSERT INTO tasks(
                task_id, workspace_id, title, goal, context, allowed_paths_json,
                acceptance_criteria, test_plan, rollback_plan, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                task.task_id,
                task.workspace_id,
                task.title,
                task.goal,
                task.context,
                serde_json::to_string(&task.allowed_paths)?,
                task.acceptance_criteria,
                task.test_plan,
                task.rollback_plan,
                task.created_at,
                task.updated_at,
            ],
        )?;
        Ok(task)
    }

    pub fn list_tasks(&self) -> KruonResult<Vec<TaskRecord>> {
        let connection = self.connection.lock().expect("M1 store mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT task_id, workspace_id, title, goal, context, allowed_paths_json,
                    acceptance_criteria, test_plan, rollback_plan, created_at, updated_at
             FROM tasks ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map([], read_task)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_task(&self, task_id: &str) -> KruonResult<TaskRecord> {
        self.connection
            .lock()
            .expect("M1 store mutex poisoned")
            .query_row(
                "SELECT task_id, workspace_id, title, goal, context, allowed_paths_json,
                        acceptance_criteria, test_plan, rollback_plan, created_at, updated_at
                 FROM tasks WHERE task_id = ?1",
                params![task_id],
                read_task,
            )
            .optional()?
            .ok_or_else(|| KruonError::NotFound(format!("task:{task_id}")))
    }

    pub fn get_task_for_run(&self, run_id: &str) -> KruonResult<TaskRecord> {
        self.connection
            .lock()
            .expect("M1 store mutex poisoned")
            .query_row(
                "SELECT t.task_id, t.workspace_id, t.title, t.goal, t.context, t.allowed_paths_json,
                        t.acceptance_criteria, t.test_plan, t.rollback_plan, t.created_at, t.updated_at
                 FROM run_queue q
                 JOIN tasks t ON t.task_id = q.task_id
                 WHERE q.run_id = ?1
                 ORDER BY q.updated_at DESC
                 LIMIT 1",
                params![run_id],
                read_task,
            )
            .optional()?
            .ok_or_else(|| KruonError::NotFound(format!("task_for_run:{run_id}")))
    }

    pub fn enqueue(&self, request: EnqueueTaskRunRequest) -> KruonResult<QueueEntry> {
        self.get_task(&request.task_id)?;
        let now = chrono::Utc::now().to_rfc3339();
        let entry = QueueEntry {
            queue_id: uuid::Uuid::new_v4().to_string(),
            task_id: request.task_id,
            adapter: request.adapter,
            state: QueueState::Queued,
            run_id: None,
            timeout_ms: request.timeout_ms,
            failure_code: None,
            created_at: now.clone(),
            updated_at: now,
        };
        self.connection
            .lock()
            .expect("M1 store mutex poisoned")
            .execute(
                "INSERT INTO run_queue(
                    queue_id, task_id, adapter, state, run_id, timeout_ms, failure_code, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, NULL, ?5, NULL, ?6, ?7)",
                params![
                    entry.queue_id,
                    entry.task_id,
                    encode(&entry.adapter)?,
                    encode(&entry.state)?,
                    entry.timeout_ms.map(sql_integer).transpose()?,
                    entry.created_at,
                    entry.updated_at,
                ],
            )?;
        Ok(entry)
    }

    pub fn list_queue(&self) -> KruonResult<Vec<QueueEntry>> {
        let connection = self.connection.lock().expect("M1 store mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT queue_id, task_id, adapter, state, run_id, timeout_ms, failure_code, created_at, updated_at
             FROM run_queue ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map([], read_queue)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_queue(&self, queue_id: &str) -> KruonResult<QueueEntry> {
        self.connection
            .lock()
            .expect("M1 store mutex poisoned")
            .query_row(
                "SELECT queue_id, task_id, adapter, state, run_id, timeout_ms, failure_code, created_at, updated_at
                 FROM run_queue WHERE queue_id = ?1",
                params![queue_id],
                read_queue,
            )
            .optional()?
            .ok_or_else(|| KruonError::NotFound(format!("queue:{queue_id}")))
    }

    pub fn claim_next_queued(&self) -> KruonResult<Option<QueueEntry>> {
        let mut connection = self.connection.lock().expect("M1 store mutex poisoned");
        let transaction = connection.transaction()?;
        let entry = transaction
            .query_row(
                "SELECT queue_id, task_id, adapter, state, run_id, timeout_ms, failure_code, created_at, updated_at
                 FROM run_queue WHERE state = ?1 ORDER BY created_at ASC LIMIT 1",
                params![encode(&QueueState::Queued)?],
                read_queue,
            )
            .optional()?;
        let Some(entry) = entry else {
            transaction.rollback()?;
            return Ok(None);
        };
        let updated = transaction.execute(
            "UPDATE run_queue SET state = ?1, updated_at = ?2 WHERE queue_id = ?3 AND state = ?4",
            params![
                encode(&QueueState::Started)?,
                chrono::Utc::now().to_rfc3339(),
                entry.queue_id,
                encode(&QueueState::Queued)?,
            ],
        )?;
        if updated != 1 {
            return Err(KruonError::Conflict("queue claim lost".into()));
        }
        transaction.commit()?;
        Ok(Some(QueueEntry {
            state: QueueState::Started,
            updated_at: chrono::Utc::now().to_rfc3339(),
            ..entry
        }))
    }

    pub fn mark_queue_started(&self, queue_id: &str, run_id: &str) -> KruonResult<()> {
        self.update_queue(queue_id, QueueState::Started, Some(run_id), None)
    }

    pub fn mark_queue_failed(&self, queue_id: &str, failure_code: &str) -> KruonResult<()> {
        let changed = self
            .connection
            .lock()
            .expect("M1 store mutex poisoned")
            .execute(
                "UPDATE run_queue
                 SET state = ?1, run_id = NULL, failure_code = ?2, updated_at = ?3
                 WHERE queue_id = ?4",
                params![
                    encode(&QueueState::Failed)?,
                    failure_code,
                    chrono::Utc::now().to_rfc3339(),
                    queue_id,
                ],
            )?;
        if changed == 0 {
            return Err(KruonError::NotFound(format!("queue:{queue_id}")));
        }
        Ok(())
    }

    /// A process that stopped after reserving a queue slot but before binding a
    /// run id could not have launched a managed process. Make it eligible for
    /// a clean retry during startup recovery.
    pub fn requeue_unbound_starts(&self) -> KruonResult<usize> {
        let changed = self
            .connection
            .lock()
            .expect("M1 store mutex poisoned")
            .execute(
                "UPDATE run_queue
                 SET state = ?1, updated_at = ?2
                 WHERE state = ?3 AND run_id IS NULL",
                params![
                    encode(&QueueState::Queued)?,
                    chrono::Utc::now().to_rfc3339(),
                    encode(&QueueState::Started)?,
                ],
            )?;
        Ok(changed)
    }

    fn update_queue(
        &self,
        queue_id: &str,
        state: QueueState,
        run_id: Option<&str>,
        failure_code: Option<&str>,
    ) -> KruonResult<()> {
        let changed = self
            .connection
            .lock()
            .expect("M1 store mutex poisoned")
            .execute(
                "UPDATE run_queue
                 SET state = ?1, run_id = COALESCE(?2, run_id), failure_code = ?3, updated_at = ?4
                 WHERE queue_id = ?5",
                params![
                    encode(&state)?,
                    run_id,
                    failure_code,
                    chrono::Utc::now().to_rfc3339(),
                    queue_id,
                ],
            )?;
        if changed == 0 {
            return Err(KruonError::NotFound(format!("queue:{queue_id}")));
        }
        Ok(())
    }
}

pub fn probe_connections() -> Vec<AdapterConnection> {
    [AdapterKind::Codex, AdapterKind::Claude]
        .into_iter()
        .map(probe_connection)
        .collect()
}

pub(crate) fn probe_connection(adapter: AdapterKind) -> AdapterConnection {
    let (approval_mode, capabilities, auth_args): (&str, Vec<&str>, &[&str]) = match adapter {
        AdapterKind::Codex => (
            "sandbox_policy_only",
            vec!["read_only", "stream_events", "cancel", "replay"],
            &["login", "status"],
        ),
        AdapterKind::Claude => (
            "sandbox_policy_only",
            vec!["read_only", "stream_events", "cancel", "replay"],
            &["auth", "status"],
        ),
    };
    let command = adapter.as_str();
    let Some(program) = resolve_adapter_program(adapter) else {
        return AdapterConnection {
            adapter,
            command: command.to_owned(),
            status: ConnectionStatus::NotFound,
            version: None,
            normalized_version: None,
            compatibility: CompatibilityStatus::Unverified,
            supported_versions: supported_versions(adapter)
                .iter()
                .map(|version| (*version).to_owned())
                .collect(),
            authentication: AuthenticationStatus::Unknown,
            approval_mode: approval_mode.to_owned(),
            capabilities: capabilities.into_iter().map(str::to_owned).collect(),
            detail: format!(
                "{command} was not found in PATH or common per-user installation locations"
            ),
        };
    };
    let version = run_probe(&program.executable, version_probe_args(adapter));
    let (status, version, normalized_version, compatibility, supported_versions, detail) =
        match version {
            Ok(output) if !output.trim().is_empty() => {
                let version = first_line(&output);
                let report = evaluate_version_output(adapter, &version);
                let status = if report.status == CompatibilityStatus::Supported {
                    ConnectionStatus::Ready
                } else {
                    ConnectionStatus::UnsupportedVersion
                };
                let detail = if report.status == CompatibilityStatus::Supported {
                    let permission_boundary = if adapter == AdapterKind::Claude {
                        "; manual permission transport is not enabled in the frozen plan"
                    } else {
                        ""
                    };
                    format!(
                        "resolved via {}; version is covered by the Alpha fixture matrix{}",
                        program.source, permission_boundary
                    )
                } else {
                    format!(
                    "resolved via {}; launch blocked because the installed version is outside the Alpha fixture matrix",
                    program.source
                )
                };
                (
                    status,
                    Some(version),
                    report.normalized_version,
                    report.status,
                    report.supported_versions,
                    detail,
                )
            }
            Ok(_) => (
                ConnectionStatus::VersionCheckFailed,
                None,
                None,
                CompatibilityStatus::Unverified,
                supported_versions(adapter)
                    .iter()
                    .map(|version| (*version).to_owned())
                    .collect(),
                format!(
                    "resolved via {}; version probe returned no version text",
                    program.source
                ),
            ),
            Err(error) => (
                ConnectionStatus::VersionCheckFailed,
                None,
                None,
                CompatibilityStatus::Unverified,
                supported_versions(adapter)
                    .iter()
                    .map(|version| (*version).to_owned())
                    .collect(),
                format!(
                    "resolved via {}; {}",
                    program.source,
                    probe_error_detail(&error)
                ),
            ),
        };
    let authentication = if status == ConnectionStatus::Ready {
        classify_auth(run_probe(&program.executable, auth_args))
    } else {
        AuthenticationStatus::Unknown
    };
    AdapterConnection {
        adapter,
        command: command.to_owned(),
        status,
        version,
        normalized_version,
        compatibility,
        supported_versions,
        authentication,
        approval_mode: approval_mode.to_owned(),
        capabilities: capabilities.into_iter().map(str::to_owned).collect(),
        detail,
    }
}

pub(crate) fn ensure_adapter_compatible(adapter: AdapterKind) -> KruonResult<()> {
    let connection = probe_connection(adapter);
    match connection.status {
        ConnectionStatus::Ready => Ok(()),
        ConnectionStatus::NotFound => Err(KruonError::Adapter(format!(
            "{} CLI was not found",
            adapter.as_str()
        ))),
        ConnectionStatus::VersionCheckFailed | ConnectionStatus::UnsupportedVersion => {
            Err(KruonError::Compatibility(format!(
                "{} version is not covered by the Alpha compatibility matrix",
                adapter.as_str()
            )))
        }
    }
}

/// Codex's top-level `--version` opens the interactive entrypoint and rejects
/// a detached stdin. `exec --version` is its non-interactive equivalent and is
/// therefore safe for the deliberately stdin-less connection probe.
fn version_probe_args(adapter: AdapterKind) -> &'static [&'static str] {
    match adapter {
        AdapterKind::Codex => &["exec", "--version"],
        AdapterKind::Claude => &["--version"],
    }
}

#[derive(Debug)]
enum ProbeError {
    NotFound,
    Failed {
        exit_code: Option<i32>,
        diagnostic: ProbeDiagnostic,
    },
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeDiagnostic {
    RequiresTerminal,
    NodeRuntime,
    NotExecutable,
    Other,
}

fn run_probe(command: &Path, args: &[&str]) -> Result<String, ProbeError> {
    let mut child = Command::new(command)
        .args(args)
        .env_clear()
        .envs(adapter_environment(command))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ProbeError::NotFound
            } else {
                ProbeError::Failed {
                    exit_code: None,
                    diagnostic: classify_probe_diagnostic(&error.to_string()),
                }
            }
        })?;
    let deadline = Instant::now() + VERSION_PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let output = child.wait_with_output().map_err(|_| ProbeError::Failed {
                    exit_code: None,
                    diagnostic: ProbeDiagnostic::Other,
                })?;
                return Ok(successful_probe_text(&output.stdout, &output.stderr));
            }
            Ok(Some(status)) => {
                let output = child.wait_with_output().map_err(|_| ProbeError::Failed {
                    exit_code: status.code(),
                    diagnostic: ProbeDiagnostic::Other,
                })?;
                return Err(ProbeError::Failed {
                    exit_code: status.code(),
                    diagnostic: classify_probe_diagnostic(&String::from_utf8_lossy(&output.stderr)),
                });
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ProbeError::TimedOut);
            }
            Err(_) => {
                return Err(ProbeError::Failed {
                    exit_code: None,
                    diagnostic: ProbeDiagnostic::Other,
                });
            }
        }
    }
}

fn successful_probe_text(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout).trim().to_owned();
    if stdout.is_empty() {
        String::from_utf8_lossy(stderr).trim().to_owned()
    } else {
        stdout
    }
}

fn classify_probe_diagnostic(stderr: &str) -> ProbeDiagnostic {
    let value = stderr.to_ascii_lowercase();
    if value.contains("stdin is not a terminal") {
        ProbeDiagnostic::RequiresTerminal
    } else if value.contains("node") {
        ProbeDiagnostic::NodeRuntime
    } else if value.contains("access is denied") || value.contains("permission denied") {
        ProbeDiagnostic::NotExecutable
    } else {
        ProbeDiagnostic::Other
    }
}

fn probe_error_detail(error: &ProbeError) -> String {
    match error {
        ProbeError::NotFound => "version probe program was not found".into(),
        ProbeError::TimedOut => "version probe timed out".into(),
        ProbeError::Failed {
            exit_code,
            diagnostic,
        } => {
            let reason = match diagnostic {
                ProbeDiagnostic::RequiresTerminal => "version probe requires a terminal",
                ProbeDiagnostic::NodeRuntime => "version probe could not start its Node runtime",
                ProbeDiagnostic::NotExecutable => "resolved program is not executable",
                ProbeDiagnostic::Other => "version probe failed",
            };
            match exit_code {
                Some(code) => format!("{reason} (exit code {code})"),
                None => reason.into(),
            }
        }
    }
}

fn classify_auth(result: Result<String, ProbeError>) -> AuthenticationStatus {
    let Ok(output) = result else {
        return AuthenticationStatus::Unknown;
    };
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&output) {
        if let Some(logged_in) = value.get("loggedIn").and_then(serde_json::Value::as_bool) {
            return if logged_in {
                AuthenticationStatus::Authenticated
            } else {
                AuthenticationStatus::Unauthenticated
            };
        }
    }
    let normalized = output.to_ascii_lowercase();
    if normalized.contains("not logged")
        || normalized.contains("not authenticated")
        || normalized.contains("login required")
        || normalized.contains("unauthenticated")
    {
        AuthenticationStatus::Unauthenticated
    } else if normalized.contains("logged in")
        || normalized.contains("authenticated")
        || normalized.contains("signed in")
    {
        AuthenticationStatus::Authenticated
    } else {
        AuthenticationStatus::Unknown
    }
}

pub fn task_prompt(task: &TaskRecord) -> String {
    let scopes = task.allowed_paths.join(", ");
    format!(
        "You are running in a read-only planning mode.\n\nGoal:\n{}\n\nContext:\n{}\n\nAllowed workspace scope:\n{}\n\nAcceptance criteria:\n{}\n\nRequested tests or checks:\n{}\n\nRollback notes:\n{}\n\nDo not modify files. Report findings and proposed next steps.",
        task.goal, task.context, scopes, task.acceptance_criteria, task.test_plan, task.rollback_plan
    )
}

pub fn validate_task_scopes(workspace: &WorkspaceRecord, task: &TaskRecord) -> KruonResult<()> {
    let root = PathBuf::from(&workspace.root);
    for scope in &task.allowed_paths {
        let candidate = if Path::new(scope).is_absolute() {
            PathBuf::from(scope)
        } else {
            root.join(scope)
        };
        let canonical = candidate.canonicalize().map_err(|_| {
            KruonError::PathPolicy("task scope must resolve inside the trusted workspace".into())
        })?;
        if !canonical.starts_with(&root) {
            return Err(KruonError::PathPolicy(
                "task scope is outside the trusted workspace".into(),
            ));
        }
    }
    Ok(())
}

fn canonical_workspace_root(root: &str) -> KruonResult<PathBuf> {
    let path = Path::new(root);
    if root.trim().is_empty() {
        return Err(KruonError::InvalidArgument(
            "workspace root must not be empty".into(),
        ));
    }
    let canonical = path.canonicalize().map_err(|_| {
        KruonError::PathPolicy("workspace root must be an existing directory".into())
    })?;
    if !canonical.is_dir() {
        return Err(KruonError::PathPolicy(
            "workspace root must be a directory".into(),
        ));
    }
    Ok(canonical)
}

fn normalize_scopes(values: &[String]) -> KruonResult<Vec<String>> {
    let values = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Ok(vec![".".to_owned()]);
    }
    if values.iter().any(|value| value.contains("..")) {
        return Err(KruonError::InvalidArgument(
            "task scope must not contain parent traversal".into(),
        ));
    }
    Ok(values)
}

fn non_empty<'a>(label: &str, value: &'a str) -> KruonResult<&'a str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(KruonError::InvalidArgument(format!(
            "{label} must not be empty"
        )));
    }
    Ok(trimmed)
}

fn encode<T: Serialize>(value: &T) -> KruonResult<String> {
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

fn sql_integer(value: u64) -> KruonResult<i64> {
    i64::try_from(value)
        .map_err(|_| KruonError::InvalidArgument("timeout exceeds SQLite integer range".into()))
}

fn read_workspace(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceRecord> {
    Ok(WorkspaceRecord {
        workspace_id: row.get(0)?,
        root: row.get(1)?,
        display_name: row.get(2)?,
        trusted: row.get::<_, i64>(3)? != 0,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn read_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRecord> {
    let scopes: String = row.get(5)?;
    Ok(TaskRecord {
        task_id: row.get(0)?,
        workspace_id: row.get(1)?,
        title: row.get(2)?,
        goal: row.get(3)?,
        context: row.get(4)?,
        allowed_paths: decode(scopes, 5)?,
        acceptance_criteria: row.get(6)?,
        test_plan: row.get(7)?,
        rollback_plan: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn read_queue(row: &rusqlite::Row<'_>) -> rusqlite::Result<QueueEntry> {
    let adapter: String = row.get(2)?;
    let state: String = row.get(3)?;
    let timeout: Option<i64> = row.get(5)?;
    Ok(QueueEntry {
        queue_id: row.get(0)?,
        task_id: row.get(1)?,
        adapter: decode(adapter, 2)?,
        state: decode(state, 3)?,
        run_id: row.get(4)?,
        timeout_ms: timeout
            .map(|value| u64::try_from(value))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Integer,
                    Box::new(error),
                )
            })?,
        failure_code: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn first_line(value: &str) -> String {
    value.lines().next().unwrap_or_default().trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_tasks_and_queue_persist_in_the_control_store() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("m1.sqlite3");
        let store = M1Store::open(&database).unwrap();
        let workspace = store
            .create_workspace(WorkspaceCreateRequest {
                root: root.path().to_string_lossy().into_owned(),
                display_name: "Fixture".into(),
            })
            .unwrap();
        assert!(!workspace.trusted);
        let workspace = store
            .set_workspace_trust(&workspace.workspace_id, true)
            .unwrap();
        assert!(workspace.trusted);
        let task = store
            .create_task(TaskCreateRequest {
                workspace_id: workspace.workspace_id,
                title: "Read the project".into(),
                goal: "Summarize the project".into(),
                context: "Use the repository only".into(),
                allowed_paths: vec![".".into()],
                acceptance_criteria: "A concise summary".into(),
                test_plan: "No tests".into(),
                rollback_plan: "No changes".into(),
            })
            .unwrap();
        let queue = store
            .enqueue(EnqueueTaskRunRequest {
                task_id: task.task_id,
                adapter: AdapterKind::Codex,
                timeout_ms: Some(2_000),
            })
            .unwrap();
        let claimed = store.claim_next_queued().unwrap().unwrap();
        assert_eq!(claimed.queue_id, queue.queue_id);
        assert_eq!(claimed.state, QueueState::Started);
        store.mark_queue_started(&queue.queue_id, "run-1").unwrap();
        assert_eq!(
            store.get_queue(&queue.queue_id).unwrap().run_id.as_deref(),
            Some("run-1")
        );
        drop(store);

        let restarted = M1Store::open(&database).unwrap();
        assert_eq!(restarted.list_workspaces().unwrap().len(), 1);
        assert_eq!(restarted.list_tasks().unwrap().len(), 1);
        assert_eq!(
            restarted.list_queue().unwrap()[0].run_id.as_deref(),
            Some("run-1")
        );
    }

    #[test]
    fn startup_recovery_requeues_only_unbound_queue_reservations() {
        let root = tempfile::tempdir().unwrap();
        let store = M1Store::open_in_memory().unwrap();
        let workspace = store
            .create_workspace(WorkspaceCreateRequest {
                root: root.path().to_string_lossy().into_owned(),
                display_name: "Fixture".into(),
            })
            .unwrap();
        let task = store
            .create_task(TaskCreateRequest {
                workspace_id: workspace.workspace_id,
                title: "Recover queue reservation".into(),
                goal: "Ensure an unbound reservation can retry".into(),
                context: "".into(),
                allowed_paths: vec![".".into()],
                acceptance_criteria: "Reservation is queued again".into(),
                test_plan: "Inspect queue state".into(),
                rollback_plan: "No changes".into(),
            })
            .unwrap();
        let queue = store
            .enqueue(EnqueueTaskRunRequest {
                task_id: task.task_id,
                adapter: AdapterKind::Codex,
                timeout_ms: None,
            })
            .unwrap();
        store.claim_next_queued().unwrap();
        assert_eq!(
            store.get_queue(&queue.queue_id).unwrap().state,
            QueueState::Started
        );

        assert_eq!(store.requeue_unbound_starts().unwrap(), 1);
        assert_eq!(
            store.get_queue(&queue.queue_id).unwrap().state,
            QueueState::Queued
        );
    }

    #[test]
    fn task_scope_rejects_parent_traversal() {
        assert!(matches!(
            normalize_scopes(&["../outside".into()]),
            Err(KruonError::InvalidArgument(_))
        ));
    }

    #[test]
    fn auth_classification_never_treats_unknown_output_as_authenticated() {
        assert_eq!(
            classify_auth(Ok("status is unavailable".into())),
            AuthenticationStatus::Unknown
        );
        assert_eq!(
            classify_auth(Ok("Not logged in".into())),
            AuthenticationStatus::Unauthenticated
        );
        assert_eq!(
            classify_auth(Ok(r#"{"loggedIn":true,"authMethod":"oauth"}"#.into())),
            AuthenticationStatus::Authenticated
        );
        assert_eq!(
            classify_auth(Ok(r#"{"loggedIn":false,"authMethod":"none"}"#.into())),
            AuthenticationStatus::Unauthenticated
        );
    }

    #[test]
    fn successful_probe_uses_stderr_only_when_stdout_is_empty() {
        assert_eq!(successful_probe_text(b"ready\n", b"ignored\n"), "ready");
        assert_eq!(
            successful_probe_text(b"", b"Logged in using ChatGPT\n"),
            "Logged in using ChatGPT"
        );
    }

    #[test]
    fn codex_uses_a_non_interactive_version_probe() {
        assert_eq!(
            version_probe_args(AdapterKind::Codex),
            &["exec", "--version"]
        );
        assert_eq!(version_probe_args(AdapterKind::Claude), &["--version"]);
    }

    #[test]
    fn probe_error_details_are_classified_without_echoing_stderr() {
        assert_eq!(
            classify_probe_diagnostic("Error: stdin is not a terminal"),
            ProbeDiagnostic::RequiresTerminal
        );
        assert_eq!(
            probe_error_detail(&ProbeError::Failed {
                exit_code: Some(1),
                diagnostic: ProbeDiagnostic::NodeRuntime,
            }),
            "version probe could not start its Node runtime (exit code 1)"
        );
    }

    #[test]
    fn version_probe_timeout_allows_for_cli_cold_start() {
        assert_eq!(VERSION_PROBE_TIMEOUT, Duration::from_secs(15));
    }

    #[cfg(windows)]
    #[test]
    fn probes_an_absolute_windows_command_shim_with_a_sanitized_environment() {
        let directory = tempfile::tempdir().unwrap();
        let shim = directory.path().join("codex.cmd");
        std::fs::write(
            &shim,
            "@echo off\r\nif \"%1\"==\"exec\" if \"%2\"==\"--version\" (\r\n  echo codex fixture 1.0\r\n  exit /b 0\r\n)\r\nexit /b 7\r\n",
        )
        .unwrap();
        assert_eq!(
            run_probe(&shim, version_probe_args(AdapterKind::Codex)).unwrap(),
            "codex fixture 1.0"
        );
    }
}
