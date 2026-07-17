use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::domain::{AdapterKind, EventEnvelope, EventPhase, RunSnapshot, TerminalState};
use super::error::{KruonError, KruonResult};
use super::m1::{
    AdapterConnection, AuthenticationStatus, ConnectionStatus, QueueEntry, QueueState, TaskRecord,
    WorkspaceRecord,
};
use super::m4::CompatibilityStatus;

pub const MAX_DIAGNOSTIC_RUNS: usize = 50;
const MAX_DIAGNOSTIC_BUNDLE_BYTES: usize = 1024 * 1024;
const DIAGNOSTIC_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLocation {
    Downloads,
    AppData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticExportRecord {
    pub file_name: String,
    pub saved_in: DiagnosticLocation,
    pub byte_count: u64,
    pub sha256: String,
    pub generated_at: String,
    pub included_runs: usize,
    pub total_runs: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticBundle {
    schema_version: u32,
    generated_at: String,
    app_version: &'static str,
    target_os: &'static str,
    target_arch: &'static str,
    database_schema_versions: Vec<i64>,
    counts: DiagnosticCounts,
    connections: Vec<ConnectionDiagnostic>,
    runs: Vec<RunDiagnostic>,
    privacy: PrivacyDiagnostic,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticCounts {
    workspaces: usize,
    trusted_workspaces: usize,
    tasks: usize,
    queue_entries: usize,
    queue_by_state: BTreeMap<&'static str, usize>,
    runs: usize,
    runs_included: usize,
    runs_truncated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionDiagnostic {
    adapter: AdapterKind,
    status: ConnectionStatus,
    normalized_version: Option<String>,
    compatibility: CompatibilityStatus,
    authentication: AuthenticationStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunDiagnostic {
    ordinal: usize,
    adapter: AdapterKind,
    status: super::domain::RunStatus,
    terminal_state: Option<TerminalState>,
    last_sequence: u64,
    event_count: usize,
    event_phases: EventPhaseCounts,
    terminal_diagnostics: Option<TerminalDiagnostics>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventPhaseCounts {
    setup: usize,
    planning: usize,
    running: usize,
    tool_call: usize,
    waiting_approval: usize,
    approval_decision: usize,
    artifact: usize,
    cancelling: usize,
    terminal: usize,
    degraded: usize,
    uncertain: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalDiagnostics {
    exit_code: Option<i64>,
    forced_stop_required: Option<bool>,
    residual_detected: Option<bool>,
    stdout_line_count: Option<u64>,
    stderr_line_count: Option<u64>,
    stdout_truncated: Option<bool>,
    stderr_truncated: Option<bool>,
    stdout_lossy_lines: Option<u64>,
    stderr_lossy_lines: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PrivacyDiagnostic {
    metadata_only: bool,
    sensitive_data_included: bool,
    secondary_scan_passed: bool,
}

pub(crate) struct DiagnosticRunSource {
    pub run: RunSnapshot,
    pub events: Vec<EventEnvelope>,
}

pub(crate) struct DiagnosticSourceSnapshot {
    pub connections: Vec<AdapterConnection>,
    pub workspaces: Vec<WorkspaceRecord>,
    pub tasks: Vec<TaskRecord>,
    pub queue: Vec<QueueEntry>,
    pub runs: Vec<DiagnosticRunSource>,
    pub total_runs: usize,
    pub database_schema_versions: Vec<i64>,
}

pub(crate) fn export_diagnostic_bundle(
    directory: &Path,
    saved_in: DiagnosticLocation,
    source: DiagnosticSourceSnapshot,
) -> KruonResult<DiagnosticExportRecord> {
    let bundle = build_bundle(source);
    let value = serde_json::to_value(&bundle)?;
    validate_bundle_value(&value)?;
    let body = serde_json::to_vec_pretty(&bundle)?;
    if body.len() > MAX_DIAGNOSTIC_BUNDLE_BYTES {
        return Err(KruonError::InvalidArgument(
            "diagnostic metadata exceeds the export size limit".into(),
        ));
    }

    fs::create_dir_all(directory)?;
    let generated_at = bundle.generated_at.clone();
    let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let suffix = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let file_name = format!("kruon-diagnostics-{timestamp}-{suffix}.json");
    let final_path = directory.join(&file_name);
    let temporary_path = directory.join(format!(".{file_name}.tmp"));

    let write_result = (|| -> KruonResult<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary_path)?;
        file.write_all(&body)?;
        file.sync_all()?;
        fs::rename(&temporary_path, &final_path)?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    write_result?;

    Ok(DiagnosticExportRecord {
        file_name,
        saved_in,
        byte_count: body.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&body)),
        generated_at,
        included_runs: bundle.runs.len(),
        total_runs: bundle.counts.runs,
    })
}

fn build_bundle(source: DiagnosticSourceSnapshot) -> DiagnosticBundle {
    let mut queue_by_state =
        BTreeMap::from([("failed", 0usize), ("queued", 0usize), ("started", 0usize)]);
    for entry in &source.queue {
        let key = match entry.state {
            QueueState::Queued => "queued",
            QueueState::Started => "started",
            QueueState::Failed => "failed",
        };
        *queue_by_state.entry(key).or_default() += 1;
    }

    let connections = source
        .connections
        .into_iter()
        .map(|connection| ConnectionDiagnostic {
            adapter: connection.adapter,
            status: connection.status,
            normalized_version: connection
                .normalized_version
                .filter(|version| is_plain_semver(version)),
            compatibility: connection.compatibility,
            authentication: connection.authentication,
        })
        .collect();

    let runs = source
        .runs
        .into_iter()
        .take(MAX_DIAGNOSTIC_RUNS)
        .enumerate()
        .map(|(index, source)| run_diagnostic(index + 1, source))
        .collect::<Vec<_>>();
    let runs_included = runs.len();

    DiagnosticBundle {
        schema_version: DIAGNOSTIC_SCHEMA_VERSION,
        generated_at: chrono::Utc::now().to_rfc3339(),
        app_version: env!("CARGO_PKG_VERSION"),
        target_os: std::env::consts::OS,
        target_arch: std::env::consts::ARCH,
        database_schema_versions: source.database_schema_versions,
        counts: DiagnosticCounts {
            workspaces: source.workspaces.len(),
            trusted_workspaces: source
                .workspaces
                .iter()
                .filter(|workspace| workspace.trusted)
                .count(),
            tasks: source.tasks.len(),
            queue_entries: source.queue.len(),
            queue_by_state,
            runs: source.total_runs,
            runs_included,
            runs_truncated: source.total_runs > runs_included,
        },
        connections,
        runs,
        privacy: PrivacyDiagnostic {
            metadata_only: true,
            sensitive_data_included: false,
            secondary_scan_passed: true,
        },
    }
}

fn run_diagnostic(ordinal: usize, source: DiagnosticRunSource) -> RunDiagnostic {
    let mut phases = EventPhaseCounts::default();
    for event in &source.events {
        match event.phase {
            EventPhase::Setup => phases.setup += 1,
            EventPhase::Planning => phases.planning += 1,
            EventPhase::Running => phases.running += 1,
            EventPhase::ToolCall => phases.tool_call += 1,
            EventPhase::WaitingApproval => phases.waiting_approval += 1,
            EventPhase::ApprovalDecision => phases.approval_decision += 1,
            EventPhase::Artifact => phases.artifact += 1,
            EventPhase::Cancelling => phases.cancelling += 1,
            EventPhase::Terminal => phases.terminal += 1,
            EventPhase::Degraded => phases.degraded += 1,
            EventPhase::Uncertain => phases.uncertain += 1,
        }
    }
    let terminal_diagnostics = source
        .events
        .iter()
        .rev()
        .find(|event| event.event_type == "run.terminal")
        .map(|event| TerminalDiagnostics {
            exit_code: event.payload.get("exit_code").and_then(Value::as_i64),
            forced_stop_required: event
                .payload
                .get("forced_stop_required")
                .and_then(Value::as_bool),
            residual_detected: event
                .payload
                .get("residual_detected")
                .and_then(Value::as_bool),
            stdout_line_count: event
                .payload
                .get("stdout_line_count")
                .and_then(Value::as_u64),
            stderr_line_count: event
                .payload
                .get("stderr_line_count")
                .and_then(Value::as_u64),
            stdout_truncated: event
                .payload
                .get("stdout_truncated")
                .and_then(Value::as_bool),
            stderr_truncated: event
                .payload
                .get("stderr_truncated")
                .and_then(Value::as_bool),
            stdout_lossy_lines: event
                .payload
                .get("stdout_lossy_lines")
                .and_then(Value::as_u64),
            stderr_lossy_lines: event
                .payload
                .get("stderr_lossy_lines")
                .and_then(Value::as_u64),
        });

    RunDiagnostic {
        ordinal,
        adapter: source.run.adapter,
        status: source.run.status,
        terminal_state: source.run.terminal_state,
        last_sequence: source.run.last_sequence,
        event_count: source.events.len(),
        event_phases: phases,
        terminal_diagnostics,
    }
}

fn is_plain_semver(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

pub(crate) fn validate_bundle_value(value: &Value) -> KruonResult<()> {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if is_forbidden_diagnostic_key(key) {
                    return Err(KruonError::InvalidArgument(
                        "diagnostic privacy validation rejected a forbidden field".into(),
                    ));
                }
                validate_bundle_value(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_bundle_value(value)?;
            }
        }
        Value::String(value) => {
            if looks_like_secret(value) || contains_absolute_path(value) {
                return Err(KruonError::InvalidArgument(
                    "diagnostic privacy validation rejected sensitive content".into(),
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

fn is_forbidden_diagnostic_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    [
        "authorization",
        "command",
        "content",
        "context",
        "credential",
        "detail",
        "display_name",
        "environment",
        "goal",
        "hash",
        "key",
        "log",
        "path",
        "pid",
        "prompt",
        "project",
        "raw",
        "secret",
        "title",
        "token",
    ]
    .iter()
    .any(|forbidden| normalized.contains(forbidden))
}

fn looks_like_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "bearer ",
        "-----begin ",
        "aiza",
        "akia",
        "gho_",
        "github_pat_",
        "ghp_",
        "ghr_",
        "ghs_",
        "ghu_",
        "glpat-",
        "sk-",
        "xoxa-",
        "xoxb-",
        "xoxp-",
        "xoxr-",
        "xoxs-",
        "ya29.",
        "api_key=",
        "apikey=",
        "access_token=",
        "authorization=",
        "credential=",
        "password=",
        "secret=",
        "token=",
    ]
    .iter()
    .any(|pattern| lower.contains(pattern))
}

fn contains_absolute_path(value: &str) -> bool {
    // No allowlisted diagnostic string requires a filesystem separator. Rejecting both
    // separators is deliberately stricter than trying to enumerate every platform's roots.
    value.contains('/') || value.contains('\\')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::{RunStatus, TerminalState};
    use crate::core::m1::{QueueState, WorkspaceRecord};

    fn sensitive_source() -> DiagnosticSourceSnapshot {
        let project_name = "Project-Orchid-Private";
        let workspace_root = r"C:\Users\Izzy\Documents\Project-Orchid-Private";
        let prompt = "Never export this private prompt sentence.";
        let run = RunSnapshot {
            run_id: "private-run-id".into(),
            adapter: AdapterKind::Codex,
            workspace_root: workspace_root.into(),
            working_directory: workspace_root.into(),
            policy_id: Some("workspace:private-workspace-id:read_only".into()),
            status: RunStatus::Failed,
            terminal_state: Some(TerminalState::Failed),
            created_at: "2026-07-17T00:00:00Z".into(),
            updated_at: "2026-07-17T00:00:01Z".into(),
            last_sequence: 1,
            prompt_hash: "private-prompt-hash".into(),
            launch_fingerprint: "private-launch-fingerprint".into(),
            pid: Some(1234),
            pgid: Some(1234),
        };
        let event = EventEnvelope::new(
            "private-run-id",
            1,
            "run.terminal",
            EventPhase::Terminal,
            Some(TerminalState::Failed),
            serde_json::json!({
                "exit_code": 7,
                "stdout_line_count": 2,
                "stdout_truncated": true,
                "api_key": "sk-proj-private-token-value",
                "prompt": prompt,
                "workspace": workspace_root,
                "raw_log": "private raw log",
            }),
        );
        DiagnosticSourceSnapshot {
            connections: vec![AdapterConnection {
                adapter: AdapterKind::Codex,
                command: format!(r"{workspace_root}\node_modules\codex.cmd"),
                status: ConnectionStatus::Ready,
                version: Some("codex-cli 0.144.2 private output".into()),
                normalized_version: Some("0.144.2".into()),
                compatibility: CompatibilityStatus::Supported,
                supported_versions: vec!["0.144.2".into()],
                authentication: AuthenticationStatus::Authenticated,
                approval_mode: "sandbox_policy_only".into(),
                capabilities: vec!["read_only".into()],
                detail: "Bearer private-auth-token".into(),
            }],
            workspaces: vec![WorkspaceRecord {
                workspace_id: "private-workspace-id".into(),
                root: workspace_root.into(),
                display_name: project_name.into(),
                trusted: true,
                created_at: "2026-07-17T00:00:00Z".into(),
                updated_at: "2026-07-17T00:00:00Z".into(),
            }],
            tasks: vec![TaskRecord {
                task_id: "private-task-id".into(),
                workspace_id: "private-workspace-id".into(),
                title: project_name.into(),
                goal: prompt.into(),
                context: "private task context".into(),
                allowed_paths: vec![workspace_root.into()],
                acceptance_criteria: "private acceptance".into(),
                test_plan: "private test plan".into(),
                rollback_plan: "private rollback plan".into(),
                created_at: "2026-07-17T00:00:00Z".into(),
                updated_at: "2026-07-17T00:00:00Z".into(),
            }],
            queue: vec![QueueEntry {
                queue_id: "private-queue-id".into(),
                task_id: "private-task-id".into(),
                adapter: AdapterKind::Codex,
                state: QueueState::Failed,
                run_id: Some("private-run-id".into()),
                timeout_ms: Some(1_000),
                failure_code: Some("private-failure-detail".into()),
                created_at: "2026-07-17T00:00:00Z".into(),
                updated_at: "2026-07-17T00:00:00Z".into(),
            }],
            runs: vec![DiagnosticRunSource {
                run,
                events: vec![event],
            }],
            total_runs: 1,
            database_schema_versions: vec![1, 2, 3, 4],
        }
    }

    #[test]
    fn metadata_allowlist_excludes_secrets_prompts_projects_paths_and_raw_logs() {
        let bundle = build_bundle(sensitive_source());
        let value = serde_json::to_value(&bundle).unwrap();
        validate_bundle_value(&value).unwrap();
        let body = serde_json::to_string(&bundle).unwrap();
        for sensitive in [
            "Project-Orchid-Private",
            r"C:\Users\Izzy",
            "Never export this private prompt sentence.",
            "sk-proj-private-token-value",
            "private raw log",
            "private-run-id",
            "private-prompt-hash",
            "private-launch-fingerprint",
            "private-auth-token",
        ] {
            assert!(!body.contains(sensitive), "bundle leaked {sensitive}");
        }
        assert!(body.contains("\"metadataOnly\":true"));
        assert!(body.contains("\"stdoutTruncated\":true"));
        assert!(body.contains("\"exitCode\":7"));
    }

    #[test]
    fn secondary_scan_rejects_forbidden_fields_secrets_and_cross_platform_paths() {
        for value in [
            serde_json::json!({"rawLog": "safe-looking"}),
            serde_json::json!({"status": "Bearer secret-value"}),
            serde_json::json!({"status": r"C:\Users\Izzy\private"}),
            serde_json::json!({"status": "/Users/izzy/private"}),
        ] {
            assert!(matches!(
                validate_bundle_value(&value),
                Err(KruonError::InvalidArgument(_))
            ));
        }
    }

    #[test]
    fn export_writes_one_bounded_atomic_json_file() {
        let directory = tempfile::tempdir().unwrap();
        let export = export_diagnostic_bundle(
            directory.path(),
            DiagnosticLocation::Downloads,
            sensitive_source(),
        )
        .unwrap();
        let path = directory.path().join(&export.file_name);
        let body = fs::read(&path).unwrap();
        assert_eq!(body.len() as u64, export.byte_count);
        assert_eq!(format!("{:x}", Sha256::digest(&body)), export.sha256);
        assert!(body.len() <= MAX_DIAGNOSTIC_BUNDLE_BYTES);
        assert_eq!(
            fs::read_dir(directory.path()).unwrap().count(),
            1,
            "temporary files must not survive a successful export"
        );
        let value: Value = serde_json::from_slice(&body).unwrap();
        validate_bundle_value(&value).unwrap();
    }
}
