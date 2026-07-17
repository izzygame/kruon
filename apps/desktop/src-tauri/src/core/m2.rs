use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::database::{ensure_supported_schema, open_local_database, run_migration};
use super::domain::{AdapterKind, RunSnapshot, TerminalState};
use super::error::{KruonError, KruonResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    PerAction,
    SandboxPolicyOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Reject,
    Narrow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequestCreate {
    pub run_id: String,
    pub task_id: String,
    pub adapter: AdapterKind,
    pub mode: ApprovalMode,
    pub action_kind: String,
    pub action_summary: String,
    pub parameters: Value,
    pub expires_in_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRecord {
    pub approval_id: String,
    pub run_id: String,
    pub task_id: String,
    pub adapter: AdapterKind,
    pub mode: ApprovalMode,
    pub action_kind: String,
    pub action_summary: String,
    pub parameter_fingerprint: String,
    pub parameters: Value,
    pub status: ApprovalStatus,
    pub expires_at: String,
    pub created_at: String,
    pub updated_at: String,
    pub supersedes_approval_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalDecisionRequest {
    pub approval_id: String,
    pub parameter_fingerprint: String,
    pub decision: ApprovalDecision,
    pub note: String,
    pub narrowed_parameters: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    File,
    Diff,
    Test,
    CompletionReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRecord {
    pub artifact_id: String,
    pub run_id: String,
    pub task_id: Option<String>,
    pub kind: ArtifactKind,
    pub path: Option<String>,
    pub in_workspace: bool,
    pub summary: String,
    pub content_sha256: Option<String>,
    pub metadata: Value,
    pub source_event_sequence: Option<u64>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactCreate {
    pub run_id: String,
    pub task_id: Option<String>,
    pub kind: ArtifactKind,
    pub path: Option<String>,
    pub in_workspace: bool,
    pub summary: String,
    pub content_sha256: Option<String>,
    pub metadata: Value,
    pub source_event_sequence: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionTestResult {
    pub name: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionReportCreate {
    pub run_id: String,
    pub task_id: String,
    pub summary: String,
    pub tests: Vec<CompletionTestResult>,
    pub changed_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskReviewStatus {
    Accepted,
    Returned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskReviewRecord {
    pub review_id: String,
    pub task_id: String,
    pub run_id: String,
    pub status: TaskReviewStatus,
    pub note: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskReviewCreate {
    pub task_id: String,
    pub run_id: String,
    pub status: TaskReviewStatus,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRecord {
    pub audit_id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub event_type: String,
    pub payload: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryAdvice {
    pub code: String,
    pub message: String,
    pub can_restart_follow_up: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseCapability {
    pub supported: bool,
    pub message: String,
}

pub struct M2Store {
    connection: Mutex<Connection>,
}

impl M2Store {
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
        connection.busy_timeout(Duration::from_secs(5))?;
        Ok(())
    }

    fn migrate(&self) -> KruonResult<()> {
        let mut connection = self.connection.lock().expect("M2 store mutex poisoned");
        run_migration(&mut connection, |transaction| {
            transaction.execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS approval_requests (
                    approval_id TEXT PRIMARY KEY,
                    run_id TEXT NOT NULL,
                    task_id TEXT NOT NULL,
                    adapter TEXT NOT NULL,
                    mode TEXT NOT NULL,
                    action_kind TEXT NOT NULL,
                    action_summary TEXT NOT NULL,
                    parameter_fingerprint TEXT NOT NULL,
                    parameters_json TEXT NOT NULL,
                    status TEXT NOT NULL,
                    expires_at TEXT NOT NULL,
                    supersedes_approval_id TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS approvals_run_status
                    ON approval_requests(run_id, status, created_at DESC);
                CREATE TABLE IF NOT EXISTS approval_decisions (
                    decision_id TEXT PRIMARY KEY,
                    approval_id TEXT NOT NULL,
                    decision TEXT NOT NULL,
                    parameter_fingerprint TEXT NOT NULL,
                    note TEXT NOT NULL,
                    narrowed_parameters_json TEXT,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS approval_decisions_approval
                    ON approval_decisions(approval_id, created_at ASC);
                CREATE TABLE IF NOT EXISTS artifacts (
                    artifact_id TEXT PRIMARY KEY,
                    run_id TEXT NOT NULL,
                    task_id TEXT,
                    kind TEXT NOT NULL,
                    path TEXT,
                    in_workspace INTEGER NOT NULL,
                    summary TEXT NOT NULL,
                    content_sha256 TEXT,
                    metadata_json TEXT NOT NULL,
                    source_event_sequence INTEGER,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS artifacts_run_created
                    ON artifacts(run_id, created_at ASC);
                CREATE TABLE IF NOT EXISTS task_reviews (
                    review_id TEXT PRIMARY KEY,
                    task_id TEXT NOT NULL,
                    run_id TEXT NOT NULL,
                    status TEXT NOT NULL,
                    note TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS task_reviews_task_created
                    ON task_reviews(task_id, created_at DESC);
                CREATE TABLE IF NOT EXISTS control_audit (
                    audit_id TEXT PRIMARY KEY,
                    entity_type TEXT NOT NULL,
                    entity_id TEXT NOT NULL,
                    event_type TEXT NOT NULL,
                    payload_json TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS control_audit_entity_created
                    ON control_audit(entity_type, entity_id, created_at ASC);
                INSERT OR IGNORE INTO schema_migrations(version, applied_at)
                    VALUES (4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));",
            )?;
            Ok(())
        })
    }

    pub fn create_approval(&self, request: ApprovalRequestCreate) -> KruonResult<ApprovalRecord> {
        if request.mode != ApprovalMode::PerAction {
            return Err(KruonError::InvalidArgument(
                "a sandbox-policy-only adapter cannot create a per-action approval".into(),
            ));
        }
        let action_kind = non_empty("approval action kind", &request.action_kind)?;
        let action_summary = bounded("approval action summary", &request.action_summary, 500)?;
        let expires = request.expires_in_seconds.unwrap_or(300).min(3_600);
        let now = Utc::now();
        let fingerprint = approval_fingerprint(
            request.adapter,
            &request.run_id,
            action_kind,
            &request.parameters,
        )?;
        let parameters = redact_value(request.parameters);
        let record = ApprovalRecord {
            approval_id: uuid::Uuid::new_v4().to_string(),
            run_id: request.run_id,
            task_id: request.task_id,
            adapter: request.adapter,
            mode: request.mode,
            action_kind: action_kind.to_owned(),
            action_summary: action_summary.to_owned(),
            parameter_fingerprint: fingerprint,
            parameters,
            status: ApprovalStatus::Pending,
            expires_at: (now + ChronoDuration::seconds(expires as i64)).to_rfc3339(),
            created_at: now.to_rfc3339(),
            updated_at: now.to_rfc3339(),
            supersedes_approval_id: None,
        };
        let connection = self.connection.lock().expect("M2 store mutex poisoned");
        insert_approval(&*connection, &record)?;
        insert_audit(
            &*connection,
            "approval",
            &record.approval_id,
            "approval.requested",
            serde_json::json!({
                "run_id": record.run_id,
                "mode": record.mode,
                "action_kind": record.action_kind,
                "parameter_fingerprint": record.parameter_fingerprint,
            }),
        )?;
        Ok(record)
    }

    pub fn list_approvals(&self, run_id: Option<&str>) -> KruonResult<Vec<ApprovalRecord>> {
        let connection = self.connection.lock().expect("M2 store mutex poisoned");
        expire_pending(&connection)?;
        let sql = if run_id.is_some() {
            "SELECT approval_id, run_id, task_id, adapter, mode, action_kind, action_summary,
                    parameter_fingerprint, parameters_json, status, expires_at,
                    supersedes_approval_id, created_at, updated_at
             FROM approval_requests WHERE run_id = ?1 ORDER BY created_at DESC"
        } else {
            "SELECT approval_id, run_id, task_id, adapter, mode, action_kind, action_summary,
                    parameter_fingerprint, parameters_json, status, expires_at,
                    supersedes_approval_id, created_at, updated_at
             FROM approval_requests ORDER BY created_at DESC"
        };
        let mut statement = connection.prepare(sql)?;
        let rows = if let Some(run_id) = run_id {
            statement.query_map(params![run_id], read_approval)?
        } else {
            statement.query_map([], read_approval)?
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn decide_approval(&self, request: ApprovalDecisionRequest) -> KruonResult<ApprovalRecord> {
        let mut connection = self.connection.lock().expect("M2 store mutex poisoned");
        let transaction = connection.transaction()?;
        let record = get_approval_in(&transaction, &request.approval_id)?;
        ensure_pending(&transaction, &record, &request.parameter_fingerprint)?;
        let note = bounded("approval decision note", &request.note, 1_000)?;
        match request.decision {
            ApprovalDecision::Approve | ApprovalDecision::Reject => {
                let status = if request.decision == ApprovalDecision::Approve {
                    ApprovalStatus::Approved
                } else {
                    ApprovalStatus::Rejected
                };
                update_approval_status(&transaction, &record.approval_id, status)?;
                insert_decision(&transaction, &record, request.decision, note, None)?;
                insert_audit(
                    &transaction,
                    "approval",
                    &record.approval_id,
                    if status == ApprovalStatus::Approved {
                        "approval.approved"
                    } else {
                        "approval.rejected"
                    },
                    serde_json::json!({"parameter_fingerprint": record.parameter_fingerprint}),
                )?;
                transaction.commit()?;
                drop(connection);
                self.get_approval(&record.approval_id)
            }
            ApprovalDecision::Narrow => {
                let narrowed = request.narrowed_parameters.as_ref().ok_or_else(|| {
                    KruonError::InvalidArgument("narrowing requires narrowed parameters".into())
                })?;
                let narrowed_fingerprint = approval_fingerprint(
                    record.adapter,
                    &record.run_id,
                    &record.action_kind,
                    narrowed,
                )?;
                let narrowed = redact_value(narrowed.clone());
                if !is_subset(&narrowed, &record.parameters) || narrowed == record.parameters {
                    return Err(KruonError::InvalidArgument(
                        "narrowed parameters must be a strict subset of the original request"
                            .into(),
                    ));
                }
                update_approval_status(
                    &transaction,
                    &record.approval_id,
                    ApprovalStatus::Superseded,
                )?;
                insert_decision(
                    &transaction,
                    &record,
                    request.decision,
                    note,
                    Some(&narrowed),
                )?;
                let now = Utc::now();
                let replacement = ApprovalRecord {
                    approval_id: uuid::Uuid::new_v4().to_string(),
                    run_id: record.run_id.clone(),
                    task_id: record.task_id.clone(),
                    adapter: record.adapter,
                    mode: record.mode,
                    action_kind: record.action_kind.clone(),
                    action_summary: record.action_summary.clone(),
                    parameter_fingerprint: narrowed_fingerprint,
                    parameters: narrowed,
                    status: ApprovalStatus::Pending,
                    expires_at: record.expires_at.clone(),
                    created_at: now.to_rfc3339(),
                    updated_at: now.to_rfc3339(),
                    supersedes_approval_id: Some(record.approval_id.clone()),
                };
                insert_approval(&transaction, &replacement)?;
                insert_audit(
                    &transaction,
                    "approval",
                    &record.approval_id,
                    "approval.narrowed",
                    serde_json::json!({
                        "replacement_approval_id": replacement.approval_id,
                        "original_fingerprint": record.parameter_fingerprint,
                        "replacement_fingerprint": replacement.parameter_fingerprint,
                    }),
                )?;
                transaction.commit()?;
                Ok(replacement)
            }
        }
    }

    pub fn get_approval(&self, approval_id: &str) -> KruonResult<ApprovalRecord> {
        let connection = self.connection.lock().expect("M2 store mutex poisoned");
        expire_pending(&connection)?;
        connection
            .query_row(
                "SELECT approval_id, run_id, task_id, adapter, mode, action_kind, action_summary,
                        parameter_fingerprint, parameters_json, status, expires_at,
                        supersedes_approval_id, created_at, updated_at
                 FROM approval_requests WHERE approval_id = ?1",
                params![approval_id],
                read_approval,
            )
            .optional()?
            .ok_or_else(|| KruonError::NotFound(format!("approval:{approval_id}")))
    }

    pub fn record_artifact(&self, create: ArtifactCreate) -> KruonResult<ArtifactRecord> {
        let summary = bounded("artifact summary", &create.summary, 2_000)?;
        let metadata = redact_value(create.metadata);
        let record = ArtifactRecord {
            artifact_id: uuid::Uuid::new_v4().to_string(),
            run_id: create.run_id,
            task_id: create.task_id,
            kind: create.kind,
            path: create
                .path
                .map(|path| path.trim().to_owned())
                .filter(|path| !path.is_empty()),
            in_workspace: create.in_workspace,
            summary: summary.to_owned(),
            content_sha256: create.content_sha256,
            metadata,
            source_event_sequence: create.source_event_sequence,
            created_at: Utc::now().to_rfc3339(),
        };
        let connection = self.connection.lock().expect("M2 store mutex poisoned");
        let duplicate: Option<String> = connection
            .query_row(
                "SELECT artifact_id FROM artifacts
                 WHERE run_id = ?1 AND kind = ?2 AND COALESCE(path, '') = COALESCE(?3, '')
                       AND COALESCE(content_sha256, '') = COALESCE(?4, '')
                 LIMIT 1",
                params![
                    record.run_id,
                    encode(&record.kind)?,
                    record.path,
                    record.content_sha256,
                ],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(artifact_id) = duplicate {
            drop(connection);
            return self.get_artifact(&artifact_id);
        }
        connection.execute(
            "INSERT INTO artifacts(
                artifact_id, run_id, task_id, kind, path, in_workspace, summary,
                content_sha256, metadata_json, source_event_sequence, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                record.artifact_id,
                record.run_id,
                record.task_id,
                encode(&record.kind)?,
                record.path,
                record.in_workspace as i64,
                record.summary,
                record.content_sha256,
                serde_json::to_string(&record.metadata)?,
                record.source_event_sequence.map(sql_integer).transpose()?,
                record.created_at,
            ],
        )?;
        insert_audit(
            &*connection,
            "artifact",
            &record.artifact_id,
            "artifact.recorded",
            serde_json::json!({
                "run_id": record.run_id,
                "kind": record.kind,
                "in_workspace": record.in_workspace,
            }),
        )?;
        insert_audit(
            &*connection,
            "run",
            &record.run_id,
            "run.artifact_recorded",
            serde_json::json!({
                "artifact_id": record.artifact_id,
                "kind": record.kind,
                "source_event_sequence": record.source_event_sequence,
            }),
        )?;
        Ok(record)
    }

    pub fn record_completion_report(
        &self,
        report: CompletionReportCreate,
    ) -> KruonResult<ArtifactRecord> {
        let summary = bounded("completion report summary", &report.summary, 4_000)?;
        let tests = report
            .tests
            .into_iter()
            .map(|test| {
                Ok(CompletionTestResult {
                    name: non_empty("test result name", &test.name)?.to_owned(),
                    status: non_empty("test result status", &test.status)?.to_owned(),
                    detail: bounded("test result detail", &test.detail, 1_000)?.to_owned(),
                })
            })
            .collect::<KruonResult<Vec<_>>>()?;
        let changed_paths = normalize_relative_paths(&report.changed_paths)?;
        self.record_artifact(ArtifactCreate {
            run_id: report.run_id,
            task_id: Some(report.task_id),
            kind: ArtifactKind::CompletionReport,
            path: None,
            in_workspace: true,
            summary: summary.to_owned(),
            content_sha256: None,
            metadata: serde_json::json!({"tests": tests, "changed_paths": changed_paths}),
            source_event_sequence: None,
        })
    }

    pub fn list_artifacts(&self, run_id: &str) -> KruonResult<Vec<ArtifactRecord>> {
        let connection = self.connection.lock().expect("M2 store mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT artifact_id, run_id, task_id, kind, path, in_workspace, summary,
                    content_sha256, metadata_json, source_event_sequence, created_at
             FROM artifacts WHERE run_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = statement.query_map(params![run_id], read_artifact)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn has_completion_report(&self, run_id: &str, task_id: &str) -> KruonResult<bool> {
        let connection = self.connection.lock().expect("M2 store mutex poisoned");
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM artifacts
             WHERE run_id = ?1 AND task_id = ?2 AND kind = ?3",
            params![run_id, task_id, encode(&ArtifactKind::CompletionReport)?],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn record_task_review(&self, review: TaskReviewCreate) -> KruonResult<TaskReviewRecord> {
        let note = bounded("task review note", &review.note, 2_000)?;
        let record = TaskReviewRecord {
            review_id: uuid::Uuid::new_v4().to_string(),
            task_id: review.task_id,
            run_id: review.run_id,
            status: review.status,
            note: note.to_owned(),
            created_at: Utc::now().to_rfc3339(),
        };
        let connection = self.connection.lock().expect("M2 store mutex poisoned");
        connection.execute(
            "INSERT INTO task_reviews(review_id, task_id, run_id, status, note, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.review_id,
                record.task_id,
                record.run_id,
                encode(&record.status)?,
                record.note,
                record.created_at,
            ],
        )?;
        insert_audit(
            &*connection,
            "task",
            &record.task_id,
            match record.status {
                TaskReviewStatus::Accepted => "task.accepted",
                TaskReviewStatus::Returned => "task.returned",
            },
            serde_json::json!({"run_id": record.run_id}),
        )?;
        Ok(record)
    }

    pub fn latest_task_reviews(&self) -> KruonResult<Vec<TaskReviewRecord>> {
        let connection = self.connection.lock().expect("M2 store mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT review_id, task_id, run_id, status, note, created_at
             FROM task_reviews ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map([], read_task_review)?;
        let mut task_ids = HashSet::new();
        let mut latest = Vec::new();
        for record in rows.collect::<Result<Vec<_>, _>>()? {
            if task_ids.insert(record.task_id.clone()) {
                latest.push(record);
            }
        }
        Ok(latest)
    }

    pub fn list_audit(&self, entity_type: &str, entity_id: &str) -> KruonResult<Vec<AuditRecord>> {
        let connection = self.connection.lock().expect("M2 store mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT audit_id, entity_type, entity_id, event_type, payload_json, created_at
             FROM control_audit WHERE entity_type = ?1 AND entity_id = ?2 ORDER BY created_at ASC",
        )?;
        let rows = statement.query_map(params![entity_type, entity_id], read_audit)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn record_control_event(
        &self,
        entity_type: &str,
        entity_id: &str,
        event_type: &str,
        payload: Value,
    ) -> KruonResult<()> {
        if entity_type.trim().is_empty()
            || entity_id.trim().is_empty()
            || event_type.trim().is_empty()
        {
            return Err(KruonError::InvalidArgument(
                "control audit fields must not be empty".into(),
            ));
        }
        let connection = self.connection.lock().expect("M2 store mutex poisoned");
        insert_audit(&*connection, entity_type, entity_id, event_type, payload)
    }

    fn get_artifact(&self, artifact_id: &str) -> KruonResult<ArtifactRecord> {
        self.connection
            .lock()
            .expect("M2 store mutex poisoned")
            .query_row(
                "SELECT artifact_id, run_id, task_id, kind, path, in_workspace, summary,
                        content_sha256, metadata_json, source_event_sequence, created_at
                 FROM artifacts WHERE artifact_id = ?1",
                params![artifact_id],
                read_artifact,
            )
            .optional()?
            .ok_or_else(|| KruonError::NotFound(format!("artifact:{artifact_id}")))
    }
}

pub fn recovery_advice(run: &RunSnapshot) -> Vec<RecoveryAdvice> {
    match run.terminal_state {
        None => vec![RecoveryAdvice {
            code: "cancel_available".into(),
            message: "The Run is active. Cancellation is available; pause is not supported by the fixed noninteractive adapters.".into(),
            can_restart_follow_up: false,
        }],
        Some(TerminalState::Completed) => vec![RecoveryAdvice {
            code: "review_artifacts".into(),
            message: "Review recorded artifacts and add a completion report before accepting the Task.".into(),
            can_restart_follow_up: true,
        }],
        Some(TerminalState::Cancelled) => vec![RecoveryAdvice {
            code: "restart_follow_up".into(),
            message: "The Run was cancelled. Start a fresh read-only follow-up after reviewing partial output.".into(),
            can_restart_follow_up: true,
        }],
        Some(TerminalState::ForcedStopRequired) | Some(TerminalState::Unknown) => vec![
            RecoveryAdvice {
                code: "review_process_cleanup".into(),
                message: "Process termination was uncertain. Review local process cleanup before starting a fresh follow-up.".into(),
                can_restart_follow_up: false,
            },
            RecoveryAdvice {
                code: "restart_after_review".into(),
                message: "After cleanup review, a fresh read-only follow-up can be queued. Session resumption is not claimed.".into(),
                can_restart_follow_up: true,
            },
        ],
        Some(TerminalState::Failed) => vec![RecoveryAdvice {
            code: "inspect_failure".into(),
            message: "Inspect normalized diagnostics, correct the Task or policy, then start a fresh follow-up.".into(),
            can_restart_follow_up: true,
        }],
    }
}

pub fn pause_capability() -> PauseCapability {
    PauseCapability {
        supported: false,
        message: "Pause is unavailable for the fixed noninteractive adapters. Use cancellation, inspect diagnostics, then start a fresh follow-up.".into(),
    }
}

fn insert_approval(
    connection: &impl BorrowedConnection,
    record: &ApprovalRecord,
) -> KruonResult<()> {
    connection.borrowed_connection().execute(
        "INSERT INTO approval_requests(
            approval_id, run_id, task_id, adapter, mode, action_kind, action_summary,
            parameter_fingerprint, parameters_json, status, expires_at, supersedes_approval_id,
            created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            record.approval_id,
            record.run_id,
            record.task_id,
            encode(&record.adapter)?,
            encode(&record.mode)?,
            record.action_kind,
            record.action_summary,
            record.parameter_fingerprint,
            serde_json::to_string(&record.parameters)?,
            encode(&record.status)?,
            record.expires_at,
            record.supersedes_approval_id,
            record.created_at,
            record.updated_at,
        ],
    )?;
    Ok(())
}

trait BorrowedConnection {
    fn borrowed_connection(&self) -> &Connection;
}

impl BorrowedConnection for Connection {
    fn borrowed_connection(&self) -> &Connection {
        self
    }
}

impl<'connection> BorrowedConnection for rusqlite::Transaction<'connection> {
    fn borrowed_connection(&self) -> &Connection {
        &*self
    }
}

fn get_approval_in(
    connection: &rusqlite::Transaction<'_>,
    approval_id: &str,
) -> KruonResult<ApprovalRecord> {
    connection
        .query_row(
            "SELECT approval_id, run_id, task_id, adapter, mode, action_kind, action_summary,
                    parameter_fingerprint, parameters_json, status, expires_at,
                    supersedes_approval_id, created_at, updated_at
             FROM approval_requests WHERE approval_id = ?1",
            params![approval_id],
            read_approval,
        )
        .optional()?
        .ok_or_else(|| KruonError::NotFound(format!("approval:{approval_id}")))
}

fn ensure_pending(
    connection: &rusqlite::Transaction<'_>,
    record: &ApprovalRecord,
    expected_fingerprint: &str,
) -> KruonResult<()> {
    if record.parameter_fingerprint != expected_fingerprint {
        return Err(KruonError::Conflict(
            "approval parameters changed and require a new request".into(),
        ));
    }
    if record.status != ApprovalStatus::Pending {
        return Err(KruonError::Conflict("approval is no longer pending".into()));
    }
    if is_expired(&record.expires_at) {
        update_approval_status(connection, &record.approval_id, ApprovalStatus::Expired)?;
        insert_audit(
            connection,
            "approval",
            &record.approval_id,
            "approval.expired",
            serde_json::json!({"parameter_fingerprint": record.parameter_fingerprint}),
        )?;
        return Err(KruonError::Conflict("approval has expired".into()));
    }
    Ok(())
}

fn update_approval_status(
    connection: &impl BorrowedConnection,
    approval_id: &str,
    status: ApprovalStatus,
) -> KruonResult<()> {
    let changed = connection.borrowed_connection().execute(
        "UPDATE approval_requests SET status = ?1, updated_at = ?2 WHERE approval_id = ?3",
        params![encode(&status)?, Utc::now().to_rfc3339(), approval_id],
    )?;
    if changed != 1 {
        return Err(KruonError::NotFound(format!("approval:{approval_id}")));
    }
    Ok(())
}

fn insert_decision(
    connection: &rusqlite::Transaction<'_>,
    record: &ApprovalRecord,
    decision: ApprovalDecision,
    note: &str,
    narrowed: Option<&Value>,
) -> KruonResult<()> {
    connection.execute(
        "INSERT INTO approval_decisions(
            decision_id, approval_id, decision, parameter_fingerprint, note,
            narrowed_parameters_json, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            uuid::Uuid::new_v4().to_string(),
            record.approval_id,
            encode(&decision)?,
            record.parameter_fingerprint,
            note,
            narrowed.map(serde_json::to_string).transpose()?,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn expire_pending(connection: &Connection) -> KruonResult<()> {
    let now = Utc::now().to_rfc3339();
    let expired = {
        let mut statement = connection.prepare(
            "SELECT approval_id, parameter_fingerprint FROM approval_requests
             WHERE status = ?1 AND expires_at <= ?2",
        )?;
        let rows = statement.query_map(params![encode(&ApprovalStatus::Pending)?, now], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for (approval_id, fingerprint) in expired {
        let changed = connection.execute(
            "UPDATE approval_requests SET status = ?1, updated_at = ?2
             WHERE approval_id = ?3 AND status = ?4",
            params![
                encode(&ApprovalStatus::Expired)?,
                now,
                approval_id,
                encode(&ApprovalStatus::Pending)?,
            ],
        )?;
        if changed == 1 {
            insert_audit(
                connection,
                "approval",
                &approval_id,
                "approval.expired",
                serde_json::json!({"parameter_fingerprint": fingerprint}),
            )?;
        }
    }
    Ok(())
}

fn insert_audit(
    connection: &impl BorrowedConnection,
    entity_type: &str,
    entity_id: &str,
    event_type: &str,
    payload: Value,
) -> KruonResult<()> {
    connection.borrowed_connection().execute(
        "INSERT INTO control_audit(audit_id, entity_type, entity_id, event_type, payload_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            uuid::Uuid::new_v4().to_string(),
            entity_type,
            entity_id,
            event_type,
            serde_json::to_string(&redact_value(payload))?,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn read_approval(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApprovalRecord> {
    let adapter: String = row.get(3)?;
    let mode: String = row.get(4)?;
    let parameters: String = row.get(8)?;
    let status: String = row.get(9)?;
    Ok(ApprovalRecord {
        approval_id: row.get(0)?,
        run_id: row.get(1)?,
        task_id: row.get(2)?,
        adapter: decode(adapter, 3)?,
        mode: decode(mode, 4)?,
        action_kind: row.get(5)?,
        action_summary: row.get(6)?,
        parameter_fingerprint: row.get(7)?,
        parameters: decode(parameters, 8)?,
        status: decode(status, 9)?,
        expires_at: row.get(10)?,
        supersedes_approval_id: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn read_artifact(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArtifactRecord> {
    let kind: String = row.get(3)?;
    let metadata: String = row.get(8)?;
    let sequence: Option<i64> = row.get(9)?;
    Ok(ArtifactRecord {
        artifact_id: row.get(0)?,
        run_id: row.get(1)?,
        task_id: row.get(2)?,
        kind: decode(kind, 3)?,
        path: row.get(4)?,
        in_workspace: row.get::<_, i64>(5)? != 0,
        summary: row.get(6)?,
        content_sha256: row.get(7)?,
        metadata: decode(metadata, 8)?,
        source_event_sequence: sequence
            .map(|value| u64::try_from(value))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    9,
                    rusqlite::types::Type::Integer,
                    Box::new(error),
                )
            })?,
        created_at: row.get(10)?,
    })
}

fn read_task_review(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskReviewRecord> {
    let status: String = row.get(3)?;
    Ok(TaskReviewRecord {
        review_id: row.get(0)?,
        task_id: row.get(1)?,
        run_id: row.get(2)?,
        status: decode(status, 3)?,
        note: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn read_audit(row: &rusqlite::Row<'_>) -> rusqlite::Result<AuditRecord> {
    let payload: String = row.get(4)?;
    Ok(AuditRecord {
        audit_id: row.get(0)?,
        entity_type: row.get(1)?,
        entity_id: row.get(2)?,
        event_type: row.get(3)?,
        payload: decode(payload, 4)?,
        created_at: row.get(5)?,
    })
}

fn approval_fingerprint(
    adapter: AdapterKind,
    run_id: &str,
    action_kind: &str,
    parameters: &Value,
) -> KruonResult<String> {
    let canonical = serde_json::to_vec(&serde_json::json!({
        "adapter": adapter,
        "run_id": run_id,
        "action_kind": action_kind,
        "parameters": parameters,
    }))?;
    Ok(format!("{:x}", Sha256::digest(canonical)))
}

fn redact_value(value: Value) -> Value {
    match value {
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| {
                    let normalized = key.to_ascii_lowercase().replace('-', "_");
                    let value = if normalized.contains("token")
                        || normalized.contains("secret")
                        || normalized.contains("password")
                        || normalized.contains("credential")
                        || normalized == "api_key"
                        || normalized == "authorization"
                    {
                        Value::String("[REDACTED]".into())
                    } else {
                        redact_value(value)
                    };
                    (key, value)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(redact_value).collect()),
        other => other,
    }
}

fn is_subset(candidate: &Value, original: &Value) -> bool {
    match (candidate, original) {
        (Value::Object(candidate), Value::Object(original)) => {
            candidate.iter().all(|(key, value)| {
                original
                    .get(key)
                    .is_some_and(|original_value| is_subset(value, original_value))
            })
        }
        (Value::Array(candidate), Value::Array(original)) => candidate.iter().all(|value| {
            original
                .iter()
                .any(|original_value| value == original_value)
        }),
        _ => candidate == original,
    }
}

fn normalize_relative_paths(paths: &[String]) -> KruonResult<Vec<String>> {
    let mut normalized = Vec::new();
    for path in paths {
        let path = path.trim();
        if path.is_empty() {
            continue;
        }
        let candidate = Path::new(path);
        if candidate.is_absolute()
            || candidate
                .components()
                .any(|part| part == std::path::Component::ParentDir)
        {
            return Err(KruonError::PathPolicy(
                "completion report paths must be relative and within the task workspace".into(),
            ));
        }
        normalized.push(path.to_owned());
    }
    Ok(normalized)
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

fn bounded<'a>(label: &str, value: &'a str, maximum: usize) -> KruonResult<&'a str> {
    let value = non_empty(label, value)?;
    if value.chars().count() > maximum {
        return Err(KruonError::InvalidArgument(format!(
            "{label} exceeds the maximum length"
        )));
    }
    Ok(value)
}

fn is_expired(expires_at: &str) -> bool {
    DateTime::parse_from_rfc3339(expires_at)
        .map(|time| time.with_timezone(&Utc) <= Utc::now())
        .unwrap_or(true)
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
        .map_err(|_| KruonError::InvalidArgument("integer exceeds SQLite range".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::RunStatus;

    #[test]
    fn approval_requires_exact_fingerprint_and_a_narrowed_request_rebinds_it() {
        let store = M2Store::open_in_memory().unwrap();
        let approval = store
            .create_approval(ApprovalRequestCreate {
                run_id: "run-1".into(),
                task_id: "task-1".into(),
                adapter: AdapterKind::Claude,
                mode: ApprovalMode::PerAction,
                action_kind: "shell_command".into(),
                action_summary: "Run bounded test command".into(),
                parameters: serde_json::json!({"paths": ["src", "tests"], "network": false}),
                expires_in_seconds: Some(300),
            })
            .unwrap();
        assert!(matches!(
            store.decide_approval(ApprovalDecisionRequest {
                approval_id: approval.approval_id.clone(),
                parameter_fingerprint: "different".into(),
                decision: ApprovalDecision::Approve,
                note: "approve".into(),
                narrowed_parameters: None,
            }),
            Err(KruonError::Conflict(_))
        ));

        let narrowed = store
            .decide_approval(ApprovalDecisionRequest {
                approval_id: approval.approval_id.clone(),
                parameter_fingerprint: approval.parameter_fingerprint.clone(),
                decision: ApprovalDecision::Narrow,
                note: "Only inspect source".into(),
                narrowed_parameters: Some(serde_json::json!({"paths": ["src"], "network": false})),
            })
            .unwrap();
        assert_eq!(
            store.get_approval(&approval.approval_id).unwrap().status,
            ApprovalStatus::Superseded
        );
        assert_ne!(
            narrowed.parameter_fingerprint,
            approval.parameter_fingerprint
        );
        let approved = store
            .decide_approval(ApprovalDecisionRequest {
                approval_id: narrowed.approval_id.clone(),
                parameter_fingerprint: narrowed.parameter_fingerprint.clone(),
                decision: ApprovalDecision::Approve,
                note: "approve narrowed request".into(),
                narrowed_parameters: None,
            })
            .unwrap();
        assert_eq!(approved.status, ApprovalStatus::Approved);
    }

    #[test]
    fn sandbox_only_approvals_are_blocked_and_reports_keep_paths_relative() {
        let store = M2Store::open_in_memory().unwrap();
        assert!(matches!(
            store.create_approval(ApprovalRequestCreate {
                run_id: "run-1".into(),
                task_id: "task-1".into(),
                adapter: AdapterKind::Codex,
                mode: ApprovalMode::SandboxPolicyOnly,
                action_kind: "file_write".into(),
                action_summary: "Write file".into(),
                parameters: serde_json::json!({"path": "src/main.rs"}),
                expires_in_seconds: None,
            }),
            Err(KruonError::InvalidArgument(_))
        ));
        assert!(matches!(
            store.record_completion_report(CompletionReportCreate {
                run_id: "run-1".into(),
                task_id: "task-1".into(),
                summary: "done".into(),
                tests: vec![],
                changed_paths: vec!["../outside".into()],
            }),
            Err(KruonError::PathPolicy(_))
        ));
    }

    #[test]
    fn expired_approvals_are_terminal_and_audited() {
        let store = M2Store::open_in_memory().unwrap();
        let approval = store
            .create_approval(ApprovalRequestCreate {
                run_id: "run-1".into(),
                task_id: "task-1".into(),
                adapter: AdapterKind::Claude,
                mode: ApprovalMode::PerAction,
                action_kind: "shell_command".into(),
                action_summary: "Run a bounded command".into(),
                parameters: serde_json::json!({"network": false}),
                expires_in_seconds: Some(0),
            })
            .unwrap();
        assert_eq!(
            store.get_approval(&approval.approval_id).unwrap().status,
            ApprovalStatus::Expired
        );
        assert!(store
            .list_audit("approval", &approval.approval_id)
            .unwrap()
            .iter()
            .any(|record| record.event_type == "approval.expired"));
    }

    #[test]
    fn terminal_recovery_advice_never_claims_session_resume() {
        let run = RunSnapshot {
            run_id: "run-1".into(),
            adapter: AdapterKind::Codex,
            workspace_root: "/tmp".into(),
            working_directory: "/tmp".into(),
            policy_id: Some("test".into()),
            status: RunStatus::Uncertain,
            terminal_state: Some(TerminalState::Unknown),
            created_at: "2026-07-15T00:00:00Z".into(),
            updated_at: "2026-07-15T00:00:00Z".into(),
            last_sequence: 1,
            prompt_hash: "hash".into(),
            launch_fingerprint: "launch".into(),
            pid: None,
            pgid: None,
        };
        assert!(recovery_advice(&run)
            .iter()
            .any(|advice| advice.message.contains("Session resumption is not claimed")));
    }
}
