use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::diagnostics::{validate_bundle_value, DiagnosticLocation};
use super::domain::{AdapterKind, RunSnapshot, TerminalState};
use super::error::{KruonError, KruonResult};
use super::m1::{
    AdapterConnection, AuthenticationStatus, ConnectionStatus, QueueEntry, QueueState, TaskRecord,
    WorkspaceRecord,
};
use super::m2::{TaskReviewRecord, TaskReviewStatus};

pub const ONBOARDING_SAMPLE_TASK_TITLE: &str = "Inspect this workspace";
pub const ONBOARDING_SAMPLE_TASK_CONTEXT: &str =
    "Kruon Alpha onboarding sample. This task is read-only and must not modify files.";
const ALPHA_METRICS_SCHEMA_VERSION: u32 = 1;
const MAX_ALPHA_METRICS_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlphaMetricsExportRecord {
    pub file_name: String,
    pub saved_in: DiagnosticLocation,
    pub byte_count: u64,
    pub sha256: String,
    pub generated_at: String,
    pub task_count: usize,
    pub run_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AlphaMetricsBundle {
    schema_version: u32,
    generated_at: String,
    app_version: &'static str,
    target_os: &'static str,
    target_arch: &'static str,
    consent: ConsentMetric,
    adapter_readiness: Vec<AdapterReadinessMetric>,
    onboarding_funnel: OnboardingFunnelMetric,
    counts: AlphaMetricCounts,
    timing: TimingMetric,
    rates: RateMetric,
    privacy: AlphaMetricsPrivacy,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConsentMetric {
    granted_for_this_export: bool,
    scope: &'static str,
    automatic_upload: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdapterReadinessMetric {
    adapter: AdapterKind,
    launch_ready: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OnboardingFunnelMetric {
    launch_ready_tool: bool,
    trusted_workspace: bool,
    read_only_sample: bool,
    sample_queued: bool,
    sample_reviewed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AlphaMetricCounts {
    workspaces: usize,
    trusted_workspaces: usize,
    tasks: usize,
    queue_entries: usize,
    queue_by_state: BTreeMap<&'static str, usize>,
    runs: usize,
    runs_by_outcome: BTreeMap<&'static str, usize>,
    reviews: usize,
    reviews_by_outcome: BTreeMap<&'static str, usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TimingMetric {
    workspace_to_first_run_seconds: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RateMetric {
    terminal_success_basis_points: Option<u32>,
    task_review_coverage_basis_points: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AlphaMetricsPrivacy {
    aggregate_only: bool,
    contains_identity: bool,
    contains_workspace_or_task_text: bool,
    contains_file_system_locations: bool,
    contains_raw_events_or_logs: bool,
    automatic_upload: bool,
}

pub(crate) struct AlphaMetricsSourceSnapshot {
    pub connections: Vec<AdapterConnection>,
    pub workspaces: Vec<WorkspaceRecord>,
    pub tasks: Vec<TaskRecord>,
    pub queue: Vec<QueueEntry>,
    pub runs: Vec<RunSnapshot>,
    pub reviews: Vec<TaskReviewRecord>,
}

pub(crate) fn export_alpha_metrics(
    directory: &Path,
    saved_in: DiagnosticLocation,
    source: AlphaMetricsSourceSnapshot,
    consented: bool,
) -> KruonResult<AlphaMetricsExportRecord> {
    if !consented {
        return Err(KruonError::InvalidArgument(
            "explicit consent is required for each Alpha metrics export".into(),
        ));
    }
    let bundle = build_bundle(source);
    let value = serde_json::to_value(&bundle)?;
    validate_bundle_value(&value)?;
    let body = serde_json::to_vec_pretty(&bundle)?;
    if body.len() > MAX_ALPHA_METRICS_BYTES {
        return Err(KruonError::InvalidArgument(
            "Alpha metrics exceed the aggregate export size limit".into(),
        ));
    }

    fs::create_dir_all(directory)?;
    let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let suffix = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let file_name = format!("kruon-alpha-metrics-{timestamp}-{suffix}.json");
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

    Ok(AlphaMetricsExportRecord {
        file_name,
        saved_in,
        byte_count: body.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&body)),
        generated_at: bundle.generated_at,
        task_count: bundle.counts.tasks,
        run_count: bundle.counts.runs,
    })
}

fn build_bundle(source: AlphaMetricsSourceSnapshot) -> AlphaMetricsBundle {
    let sample = source.tasks.iter().find(|task| {
        task.title == ONBOARDING_SAMPLE_TASK_TITLE
            && task.context == ONBOARDING_SAMPLE_TASK_CONTEXT
    });
    let sample_queue = sample.and_then(|task| {
        source
            .queue
            .iter()
            .find(|entry| entry.task_id == task.task_id)
    });
    let sample_reviewed = sample.is_some_and(|task| {
        source
            .reviews
            .iter()
            .any(|review| review.task_id == task.task_id)
    });

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

    let mut runs_by_outcome = BTreeMap::from([
        ("active", 0usize),
        ("cancelled", 0usize),
        ("completed", 0usize),
        ("failed", 0usize),
        ("forced_stop_required", 0usize),
        ("unknown", 0usize),
    ]);
    for run in &source.runs {
        let key = match run.terminal_state {
            None => "active",
            Some(TerminalState::Completed) => "completed",
            Some(TerminalState::Failed) => "failed",
            Some(TerminalState::Cancelled) => "cancelled",
            Some(TerminalState::ForcedStopRequired) => "forced_stop_required",
            Some(TerminalState::Unknown) => "unknown",
        };
        *runs_by_outcome.entry(key).or_default() += 1;
    }

    let mut reviews_by_outcome =
        BTreeMap::from([("accepted", 0usize), ("returned", 0usize)]);
    for review in &source.reviews {
        let key = match review.status {
            TaskReviewStatus::Accepted => "accepted",
            TaskReviewStatus::Returned => "returned",
        };
        *reviews_by_outcome.entry(key).or_default() += 1;
    }

    let terminal_runs = source
        .runs
        .iter()
        .filter(|run| run.terminal_state.is_some())
        .count();
    let completed_runs = source
        .runs
        .iter()
        .filter(|run| run.terminal_state == Some(TerminalState::Completed))
        .count();
    let reviewed_tasks = source
        .reviews
        .iter()
        .map(|review| &review.task_id)
        .collect::<std::collections::HashSet<_>>()
        .len();

    AlphaMetricsBundle {
        schema_version: ALPHA_METRICS_SCHEMA_VERSION,
        generated_at: chrono::Utc::now().to_rfc3339(),
        app_version: env!("CARGO_PKG_VERSION"),
        target_os: std::env::consts::OS,
        target_arch: std::env::consts::ARCH,
        consent: ConsentMetric {
            granted_for_this_export: true,
            scope: "aggregate_local_alpha_metrics",
            automatic_upload: false,
        },
        adapter_readiness: [AdapterKind::Codex, AdapterKind::Claude]
            .into_iter()
            .map(|adapter| AdapterReadinessMetric {
                adapter,
                launch_ready: source.connections.iter().any(|connection| {
                    connection.adapter == adapter
                        && connection.status == ConnectionStatus::Ready
                        && connection.authentication == AuthenticationStatus::Authenticated
                }),
            })
            .collect(),
        onboarding_funnel: OnboardingFunnelMetric {
            launch_ready_tool: source.connections.iter().any(|connection| {
                connection.status == ConnectionStatus::Ready
                    && connection.authentication == AuthenticationStatus::Authenticated
            }),
            trusted_workspace: source.workspaces.iter().any(|workspace| workspace.trusted),
            read_only_sample: sample.is_some(),
            sample_queued: sample_queue.is_some(),
            sample_reviewed,
        },
        counts: AlphaMetricCounts {
            workspaces: source.workspaces.len(),
            trusted_workspaces: source
                .workspaces
                .iter()
                .filter(|workspace| workspace.trusted)
                .count(),
            tasks: source.tasks.len(),
            queue_entries: source.queue.len(),
            queue_by_state,
            runs: source.runs.len(),
            runs_by_outcome,
            reviews: source.reviews.len(),
            reviews_by_outcome,
        },
        timing: TimingMetric {
            workspace_to_first_run_seconds: elapsed_seconds(
                source.workspaces.iter().map(|item| item.created_at.as_str()),
                source.runs.iter().map(|item| item.created_at.as_str()),
            ),
        },
        rates: RateMetric {
            terminal_success_basis_points: basis_points(completed_runs, terminal_runs),
            task_review_coverage_basis_points: basis_points(reviewed_tasks, source.tasks.len()),
        },
        privacy: AlphaMetricsPrivacy {
            aggregate_only: true,
            contains_identity: false,
            contains_workspace_or_task_text: false,
            contains_file_system_locations: false,
            contains_raw_events_or_logs: false,
            automatic_upload: false,
        },
    }
}

fn basis_points(numerator: usize, denominator: usize) -> Option<u32> {
    (denominator > 0).then(|| ((numerator * 10_000) / denominator) as u32)
}

fn elapsed_seconds<'a>(
    starts: impl Iterator<Item = &'a str>,
    ends: impl Iterator<Item = &'a str>,
) -> Option<i64> {
    let start = starts
        .filter_map(|value| DateTime::parse_from_rfc3339(value).ok())
        .min()?;
    let end = ends
        .filter_map(|value| DateTime::parse_from_rfc3339(value).ok())
        .min()?;
    (end >= start).then(|| (end - start).num_seconds())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::m1::{CompatibilityStatus, WorkspaceRecord};

    fn source() -> AlphaMetricsSourceSnapshot {
        let workspace = WorkspaceRecord {
            workspace_id: "private-workspace-id".into(),
            root: r"C:\Users\private\Project-Orchid".into(),
            display_name: "Project-Orchid".into(),
            trusted: true,
            created_at: "2026-07-17T00:00:00Z".into(),
            updated_at: "2026-07-17T00:00:00Z".into(),
        };
        let task = TaskRecord {
            task_id: "private-task-id".into(),
            workspace_id: workspace.workspace_id.clone(),
            title: ONBOARDING_SAMPLE_TASK_TITLE.into(),
            goal: "Private prompt-like goal".into(),
            context: ONBOARDING_SAMPLE_TASK_CONTEXT.into(),
            allowed_paths: vec!["secret/source".into()],
            acceptance_criteria: "Private acceptance".into(),
            test_plan: "Private test".into(),
            rollback_plan: "Private rollback".into(),
            created_at: "2026-07-17T00:00:01Z".into(),
            updated_at: "2026-07-17T00:00:01Z".into(),
        };
        AlphaMetricsSourceSnapshot {
            connections: vec![AdapterConnection {
                adapter: AdapterKind::Codex,
                command: "codex".into(),
                status: ConnectionStatus::Ready,
                version: Some("private version text".into()),
                normalized_version: Some("0.144.1".into()),
                compatibility: CompatibilityStatus::Supported,
                supported_versions: vec!["0.144.1".into()],
                authentication: AuthenticationStatus::Authenticated,
                approval_mode: "sandbox_policy_only".into(),
                capabilities: vec!["read_only".into()],
                detail: "private location detail".into(),
            }],
            workspaces: vec![workspace.clone()],
            tasks: vec![task.clone()],
            queue: vec![QueueEntry {
                queue_id: "private-queue-id".into(),
                task_id: task.task_id.clone(),
                adapter: AdapterKind::Codex,
                state: QueueState::Started,
                run_id: Some("private-run-id".into()),
                timeout_ms: Some(60_000),
                failure_code: None,
                created_at: "2026-07-17T00:00:02Z".into(),
                updated_at: "2026-07-17T00:00:02Z".into(),
            }],
            runs: vec![RunSnapshot {
                run_id: "private-run-id".into(),
                adapter: AdapterKind::Codex,
                workspace_root: workspace.root.clone(),
                working_directory: workspace.root.clone(),
                policy_id: Some("private-policy-id".into()),
                status: crate::core::domain::RunStatus::Completed,
                terminal_state: Some(TerminalState::Completed),
                created_at: "2026-07-17T00:01:00Z".into(),
                updated_at: "2026-07-17T00:02:00Z".into(),
                last_sequence: 3,
                prompt_hash: "private-prompt-hash".into(),
                launch_fingerprint: "private-launch-hash".into(),
                pid: Some(1234),
                pgid: Some(1234),
            }],
            reviews: vec![TaskReviewRecord {
                review_id: "private-review-id".into(),
                task_id: task.task_id,
                run_id: "private-run-id".into(),
                status: TaskReviewStatus::Accepted,
                note: "Private review note".into(),
                created_at: "2026-07-17T00:03:00Z".into(),
            }],
        }
    }

    #[test]
    fn aggregate_bundle_excludes_identity_content_locations_and_raw_records() {
        let bundle = build_bundle(source());
        let value = serde_json::to_value(&bundle).unwrap();
        validate_bundle_value(&value).unwrap();
        let text = serde_json::to_string(&bundle).unwrap();
        for forbidden in [
            "Project-Orchid",
            "private-workspace-id",
            "private-task-id",
            "private-run-id",
            "Private prompt-like goal",
            "secret/source",
            r"C:\Users\private",
            "private review note",
        ] {
            assert!(!text.contains(forbidden), "metrics leaked {forbidden}");
        }
        assert_eq!(bundle.counts.tasks, 1);
        assert_eq!(bundle.counts.runs, 1);
        assert_eq!(bundle.timing.workspace_to_first_run_seconds, Some(60));
        assert_eq!(bundle.rates.terminal_success_basis_points, Some(10_000));
        assert!(bundle.onboarding_funnel.sample_reviewed);
    }

    #[test]
    fn every_export_requires_consent_and_writes_one_bounded_local_file() {
        let directory = tempfile::tempdir().unwrap();
        assert!(matches!(
            export_alpha_metrics(
                directory.path(),
                DiagnosticLocation::Downloads,
                source(),
                false
            ),
            Err(KruonError::InvalidArgument(_))
        ));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);

        let record = export_alpha_metrics(
            directory.path(),
            DiagnosticLocation::Downloads,
            source(),
            true,
        )
        .unwrap();
        assert_eq!(record.task_count, 1);
        assert_eq!(record.run_count, 1);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        let value: serde_json::Value = serde_json::from_slice(
            &std::fs::read(directory.path().join(record.file_name)).unwrap(),
        )
        .unwrap();
        assert_eq!(value["consent"]["grantedForThisExport"], true);
        assert_eq!(value["consent"]["automaticUpload"], false);
        assert_eq!(value["privacy"]["aggregateOnly"], true);
    }
}
