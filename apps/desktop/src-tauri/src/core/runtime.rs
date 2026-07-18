use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::adapter_host::AdapterHost;
use super::alpha_metrics::{
    export_alpha_metrics as write_alpha_metrics, AlphaMetricsExportRecord,
    AlphaMetricsSourceSnapshot, ONBOARDING_SAMPLE_TASK_CONTEXT, ONBOARDING_SAMPLE_TASK_TITLE,
};
use super::diagnostics::{
    export_diagnostic_bundle, DiagnosticExportRecord, DiagnosticLocation, DiagnosticRunSource,
    DiagnosticSourceSnapshot, MAX_DIAGNOSTIC_RUNS,
};
use super::domain::{
    EventEnvelope, EventPhase, ReplayResult, RunSnapshot, StartRunRequest, TerminalState,
};
use super::error::{KruonError, KruonResult};
use super::event_store::EventStore;
use super::m1::{
    ensure_adapter_compatible, probe_connections as discover_connections, task_prompt,
    validate_task_scopes, AdapterConnection, EnqueueTaskRunRequest, M1Store, QueueEntry,
    TaskCreateRequest, TaskRecord, WorkspaceCreateRequest, WorkspaceRecord, MAX_CONCURRENT_RUNS,
};
use super::m2::{
    pause_capability, recovery_advice, ArtifactCreate, ArtifactKind, ArtifactRecord,
    CompletionReportCreate, M2Store, PauseCapability, RecoveryAdvice, TaskReviewCreate,
    TaskReviewRecord, TaskReviewStatus,
};
use super::m3::{project_world_station, WorldSnapshot};
use super::path_policy::PathPolicy;
use super::process_supervisor::{ProcessOutcome, ProcessSupervisor};

pub struct RuntimeCore {
    store: Arc<EventStore>,
    control: Arc<M1Store>,
    m2: Arc<M2Store>,
    supervisor: Arc<ProcessSupervisor>,
    adapter_host: AdapterHost,
    append_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    finalize_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    queue_dispatch_lock: Mutex<()>,
    sample_task_lock: Mutex<()>,
}

impl RuntimeCore {
    pub fn open(database_path: impl AsRef<Path>) -> KruonResult<Arc<Self>> {
        let database_path = database_path.as_ref();
        let store = Arc::new(EventStore::open(database_path)?);
        let control = Arc::new(M1Store::open(database_path)?);
        let m2 = Arc::new(M2Store::open(database_path)?);
        let runtime = Arc::new(Self::new_with_control(
            store,
            control,
            m2,
            Arc::new(ProcessSupervisor::default()),
        ));
        runtime.store.recover_interrupted_runs()?;
        runtime.control.requeue_unbound_starts()?;
        runtime.dispatch_queued_runs()?;
        Ok(runtime)
    }

    pub fn new(store: Arc<EventStore>, supervisor: Arc<ProcessSupervisor>) -> Self {
        let control = Arc::new(M1Store::open_in_memory().expect("M1 control store must open"));
        let m2 = Arc::new(M2Store::open_in_memory().expect("M2 control store must open"));
        Self::new_with_control(store, control, m2, supervisor)
    }

    pub fn new_with_control(
        store: Arc<EventStore>,
        control: Arc<M1Store>,
        m2: Arc<M2Store>,
        supervisor: Arc<ProcessSupervisor>,
    ) -> Self {
        Self {
            store,
            control,
            m2,
            supervisor,
            adapter_host: AdapterHost,
            append_locks: Mutex::new(HashMap::new()),
            finalize_locks: Mutex::new(HashMap::new()),
            queue_dispatch_lock: Mutex::new(()),
            sample_task_lock: Mutex::new(()),
        }
    }

    pub fn store(&self) -> Arc<EventStore> {
        Arc::clone(&self.store)
    }

    pub fn start(self: &Arc<Self>, request: StartRunRequest) -> KruonResult<RunSnapshot> {
        self.start_with_run_id(request, uuid::Uuid::new_v4().to_string())
    }

    fn start_with_run_id(
        self: &Arc<Self>,
        request: StartRunRequest,
        run_id: String,
    ) -> KruonResult<RunSnapshot> {
        ensure_adapter_compatible(request.adapter)?;
        let paths = PathPolicy::validate(&request.workspace_root, &request.working_directory)?;
        let plan = self.adapter_host.launch_plan(
            request.adapter,
            &paths.working_directory,
            &request.prompt,
        )?;
        let launch_fingerprint = plan.fingerprint();
        self.store.create_run(
            &run_id,
            &request,
            &paths.workspace_root,
            &paths.working_directory,
            &launch_fingerprint,
        )?;
        self.append_next(
            &run_id,
            "run.submitted",
            EventPhase::Setup,
            None,
            serde_json::json!({
                "adapter": request.adapter,
                "policy_id": request.policy_id,
                "prompt_hash": request.prompt_hash(),
                "launch_fingerprint": launch_fingerprint,
            }),
        )?;

        let handle = match self.supervisor.spawn(&run_id, plan) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = self.append_next(
                    &run_id,
                    "run.spawn_failed",
                    EventPhase::Terminal,
                    Some(TerminalState::Failed),
                    serde_json::json!({"error_code": public_error_code(&error)}),
                );
                return Err(error);
            }
        };
        self.store
            .update_process(&run_id, handle.pid, handle.pgid)?;
        self.append_next(
            &run_id,
            "run.started",
            EventPhase::Running,
            None,
            serde_json::json!({"pid": handle.pid, "pgid": handle.pgid}),
        )?;

        let timeout =
            Duration::from_millis(request.timeout_ms.unwrap_or(60_000).clamp(100, 3_600_000));
        let runtime = Arc::clone(self);
        let background_run_id = run_id.clone();
        std::thread::spawn(move || {
            if let Ok(outcome) = runtime.supervisor.wait(&background_run_id, timeout) {
                let _ = runtime.finalize_outcome(&background_run_id, request.adapter, outcome);
                let _ = runtime.dispatch_queued_runs();
            }
        });
        self.store.get_run(&run_id)
    }

    pub fn cancel(self: &Arc<Self>, run_id: &str) -> KruonResult<RunSnapshot> {
        let snapshot = self.store.get_run(run_id)?;
        if snapshot.terminal_state.is_some() {
            return Ok(snapshot);
        }
        self.supervisor.request_cancel(run_id)?;
        if let Err(error) = self.append_next(
            run_id,
            "run.cancel_requested",
            EventPhase::Cancelling,
            None,
            serde_json::json!({"reason": "user"}),
        ) {
            let current = self.store.get_run(run_id)?;
            if current.terminal_state.is_some() {
                return Ok(current);
            }
            return Err(error);
        }
        let outcome = self.supervisor.cancel(run_id, "user")?;
        self.finalize_outcome(run_id, snapshot.adapter, outcome)?;
        self.dispatch_queued_runs()?;
        self.store.get_run(run_id)
    }

    pub fn get_run(&self, run_id: &str) -> KruonResult<RunSnapshot> {
        self.store.get_run(run_id)
    }

    pub fn list_events(
        &self,
        run_id: &str,
        after_sequence: u64,
    ) -> KruonResult<Vec<EventEnvelope>> {
        self.store.list_events(run_id, after_sequence)
    }

    pub fn replay_run(&self, run_id: &str) -> KruonResult<ReplayResult> {
        self.store.replay_run(run_id)
    }

    pub fn list_runs(&self) -> KruonResult<Vec<RunSnapshot>> {
        self.store.list_runs()
    }

    pub fn export_diagnostic_bundle(
        &self,
        directory: &Path,
        saved_in: DiagnosticLocation,
        connections: Vec<AdapterConnection>,
    ) -> KruonResult<DiagnosticExportRecord> {
        let workspaces = self.list_workspaces()?;
        let tasks = self.list_tasks()?;
        let queue = self.list_queue()?;
        let runs = self.list_runs()?;
        let total_runs = runs.len();
        let mut diagnostic_runs = Vec::with_capacity(total_runs.min(MAX_DIAGNOSTIC_RUNS));
        for run in runs.into_iter().take(MAX_DIAGNOSTIC_RUNS) {
            let events = self.list_events(&run.run_id, 0)?;
            diagnostic_runs.push(DiagnosticRunSource { run, events });
        }
        export_diagnostic_bundle(
            directory,
            saved_in,
            DiagnosticSourceSnapshot {
                connections,
                workspaces,
                tasks,
                queue,
                runs: diagnostic_runs,
                total_runs,
                database_schema_versions: self.store.schema_versions()?,
            },
        )
    }

    pub fn export_alpha_metrics(
        &self,
        directory: &Path,
        saved_in: DiagnosticLocation,
        connections: Vec<AdapterConnection>,
        consented: bool,
    ) -> KruonResult<AlphaMetricsExportRecord> {
        write_alpha_metrics(
            directory,
            saved_in,
            AlphaMetricsSourceSnapshot {
                connections,
                workspaces: self.list_workspaces()?,
                tasks: self.list_tasks()?,
                queue: self.list_queue()?,
                runs: self.list_runs()?,
                reviews: self.latest_task_reviews()?,
            },
            consented,
        )
    }

    pub fn world_snapshot(&self) -> KruonResult<WorldSnapshot> {
        let runs = self.store.list_runs()?;
        let reviews = self.m2.latest_task_reviews()?;
        let reviews_by_run = reviews
            .into_iter()
            .map(|review| (review.run_id, review.status))
            .collect::<HashMap<_, _>>();
        let mut stations = Vec::with_capacity(2);

        for adapter in [
            super::domain::AdapterKind::Codex,
            super::domain::AdapterKind::Claude,
        ] {
            let run = runs.iter().find(|run| run.adapter == adapter);
            let events = run
                .map(|run| self.store.list_events(&run.run_id, 0))
                .transpose()?
                .unwrap_or_default();
            let review = run.and_then(|run| reviews_by_run.get(&run.run_id).copied());
            stations.push(project_world_station(adapter, run, &events, review));
        }

        Ok(WorldSnapshot {
            generated_at: chrono::Utc::now().to_rfc3339(),
            stations,
        })
    }

    pub fn create_workspace(
        &self,
        request: WorkspaceCreateRequest,
    ) -> KruonResult<WorkspaceRecord> {
        self.control.create_workspace(request)
    }

    pub fn list_workspaces(&self) -> KruonResult<Vec<WorkspaceRecord>> {
        self.control.list_workspaces()
    }

    pub fn trust_workspace(&self, workspace_id: &str) -> KruonResult<WorkspaceRecord> {
        self.control.set_workspace_trust(workspace_id, true)
    }

    pub fn untrust_workspace(&self, workspace_id: &str) -> KruonResult<WorkspaceRecord> {
        self.control.set_workspace_trust(workspace_id, false)
    }

    pub fn create_task(&self, request: TaskCreateRequest) -> KruonResult<TaskRecord> {
        self.control.create_task(request)
    }

    pub fn create_sample_task(&self, workspace_id: &str) -> KruonResult<TaskRecord> {
        let _guard = self
            .sample_task_lock
            .lock()
            .expect("sample task mutex poisoned");
        let workspace = self.control.get_workspace(workspace_id)?;
        if !workspace.trusted {
            return Err(KruonError::PathPolicy(
                "workspace must be trusted before creating the onboarding sample".into(),
            ));
        }
        if let Some(task) = self.list_tasks()?.into_iter().find(|task| {
            task.workspace_id == workspace_id
                && task.title == ONBOARDING_SAMPLE_TASK_TITLE
                && task.context == ONBOARDING_SAMPLE_TASK_CONTEXT
        }) {
            return Ok(task);
        }
        self.create_task(TaskCreateRequest {
            workspace_id: workspace_id.to_owned(),
            title: ONBOARDING_SAMPLE_TASK_TITLE.into(),
            goal: "Summarize the workspace structure and identify primary development entry points without changing files.".into(),
            context: ONBOARDING_SAMPLE_TASK_CONTEXT.into(),
            allowed_paths: vec![".".into()],
            acceptance_criteria: "Provide a concise structure summary and entry-point list; make no workspace changes.".into(),
            test_plan: "Confirm the response is read-only and no workspace files changed.".into(),
            rollback_plan: "No rollback is expected because this task must not change files.".into(),
        })
    }

    pub fn list_tasks(&self) -> KruonResult<Vec<TaskRecord>> {
        self.control.list_tasks()
    }

    pub fn list_queue(&self) -> KruonResult<Vec<QueueEntry>> {
        self.control.list_queue()
    }

    pub fn enqueue_task_run(
        self: &Arc<Self>,
        request: EnqueueTaskRunRequest,
    ) -> KruonResult<QueueEntry> {
        let task = self.control.get_task(&request.task_id)?;
        let workspace = self.control.get_workspace(&task.workspace_id)?;
        if !workspace.trusted {
            return Err(KruonError::PathPolicy(
                "workspace must be trusted before a noninteractive CLI can launch".into(),
            ));
        }
        validate_task_scopes(&workspace, &task)?;
        let entry = self.control.enqueue(request)?;
        self.dispatch_queued_runs()?;
        self.control.get_queue(&entry.queue_id)
    }

    pub fn list_approvals(
        &self,
        run_id: Option<&str>,
    ) -> KruonResult<Vec<super::m2::ApprovalRecord>> {
        self.m2.list_approvals(run_id)
    }

    pub fn list_artifacts(&self, run_id: &str) -> KruonResult<Vec<ArtifactRecord>> {
        self.m2.list_artifacts(run_id)
    }

    pub fn collect_artifacts(&self, run_id: &str) -> KruonResult<Vec<ArtifactRecord>> {
        let run = self.store.get_run(run_id)?;
        let task = self.control.get_task_for_run(run_id).ok();
        let workspace = task
            .as_ref()
            .map(|task| self.control.get_workspace(&task.workspace_id))
            .transpose()?;
        let events = self.store.list_events(run_id, 0)?;
        let mut artifacts = Vec::new();

        for event in &events {
            if let Some(workspace) = workspace.as_ref() {
                for raw_path in values_for_keys(&event.payload, &["path", "file_path"]) {
                    if let Some((path, content_sha256, byte_length)) =
                        safe_workspace_file(workspace, task.as_ref(), &raw_path)?
                    {
                        artifacts.push(self.m2.record_artifact(ArtifactCreate {
                            run_id: run_id.to_owned(),
                            task_id: task.as_ref().map(|task| task.task_id.clone()),
                            kind: ArtifactKind::File,
                            path: Some(path),
                            in_workspace: true,
                            summary: format!(
                                "Adapter-reported workspace file from event #{}",
                                event.sequence
                            ),
                            content_sha256,
                            metadata: serde_json::json!({"byte_length": byte_length}),
                            source_event_sequence: Some(event.sequence),
                        })?);
                    }
                }
            }

            for diff in values_for_keys(&event.payload, &["diff", "patch"]) {
                artifacts.push(self.m2.record_artifact(ArtifactCreate {
                    run_id: run_id.to_owned(),
                    task_id: task.as_ref().map(|task| task.task_id.clone()),
                    kind: ArtifactKind::Diff,
                    path: None,
                    in_workspace: workspace.is_some(),
                    summary: format!("Adapter diff fingerprint from event #{}", event.sequence),
                    content_sha256: Some(format!("{:x}", Sha256::digest(diff.as_bytes()))),
                    metadata: serde_json::json!({"byte_length": diff.len()}),
                    source_event_sequence: Some(event.sequence),
                })?);
            }
        }

        if let Some(terminal) = events
            .iter()
            .find(|event| event.event_type == "run.terminal")
        {
            artifacts.push(self.m2.record_artifact(ArtifactCreate {
                run_id: run_id.to_owned(),
                task_id: task.as_ref().map(|task| task.task_id.clone()),
                kind: ArtifactKind::Test,
                path: None,
                in_workspace: workspace.is_some(),
                summary: "Managed run terminal diagnostic".into(),
                content_sha256: None,
                metadata: serde_json::json!({
                    "run_status": run.status,
                    "terminal_state": run.terminal_state,
                    "exit_code": terminal.payload.get("exit_code"),
                    "stdout_line_count": terminal.payload.get("stdout_line_count"),
                    "stderr_line_count": terminal.payload.get("stderr_line_count"),
                }),
                source_event_sequence: Some(terminal.sequence),
            })?);
        }
        Ok(artifacts)
    }

    pub fn record_completion_report(
        &self,
        report: CompletionReportCreate,
    ) -> KruonResult<ArtifactRecord> {
        let (task, workspace, run) = self.task_workspace_run(&report.task_id, &report.run_id)?;
        if run.terminal_state.is_none() {
            return Err(KruonError::Conflict(
                "a completion report can only be recorded after the run is terminal".into(),
            ));
        }
        validate_report_paths(&workspace, &task, &report.changed_paths)?;
        let artifact = self.m2.record_completion_report(report)?;
        self.m2.record_control_event(
            "run",
            &run.run_id,
            "run.completion_report_recorded",
            serde_json::json!({"artifact_id": artifact.artifact_id, "task_id": task.task_id}),
        )?;
        Ok(artifact)
    }

    pub fn latest_task_reviews(&self) -> KruonResult<Vec<TaskReviewRecord>> {
        self.m2.latest_task_reviews()
    }

    pub fn list_run_audit(&self, run_id: &str) -> KruonResult<Vec<super::m2::AuditRecord>> {
        self.store.get_run(run_id)?;
        self.m2.list_audit("run", run_id)
    }

    pub fn accept_task(
        &self,
        task_id: &str,
        run_id: &str,
        note: &str,
    ) -> KruonResult<TaskReviewRecord> {
        let (task, _workspace, run) = self.task_workspace_run(task_id, run_id)?;
        if run.terminal_state != Some(TerminalState::Completed) {
            return Err(KruonError::Conflict(
                "only a completed run can be accepted; terminal status alone is not acceptance"
                    .into(),
            ));
        }
        if !self.m2.has_completion_report(run_id, task_id)? {
            return Err(KruonError::Conflict(
                "record a completion report before accepting the task".into(),
            ));
        }
        let review = self.m2.record_task_review(TaskReviewCreate {
            task_id: task.task_id,
            run_id: run.run_id,
            status: TaskReviewStatus::Accepted,
            note: if note.trim().is_empty() {
                "Accepted after artifact review".into()
            } else {
                note.to_owned()
            },
        })?;
        self.m2.record_control_event(
            "run",
            &review.run_id,
            "run.task_accepted",
            serde_json::json!({"task_id": review.task_id, "review_id": review.review_id}),
        )?;
        Ok(review)
    }

    pub fn return_task(
        &self,
        task_id: &str,
        run_id: &str,
        note: &str,
    ) -> KruonResult<TaskReviewRecord> {
        let (task, _workspace, run) = self.task_workspace_run(task_id, run_id)?;
        if run.terminal_state.is_none() {
            return Err(KruonError::Conflict(
                "a task can only be returned after the run is terminal".into(),
            ));
        }
        let review = self.m2.record_task_review(TaskReviewCreate {
            task_id: task.task_id,
            run_id: run.run_id,
            status: TaskReviewStatus::Returned,
            note: note.to_owned(),
        })?;
        self.m2.record_control_event(
            "run",
            &review.run_id,
            "run.task_returned",
            serde_json::json!({"task_id": review.task_id, "review_id": review.review_id}),
        )?;
        Ok(review)
    }

    pub fn restart_follow_up(self: &Arc<Self>, run_id: &str) -> KruonResult<QueueEntry> {
        let (task, _workspace, run) = self.task_workspace_run_for_run(run_id)?;
        if run.terminal_state.is_none() {
            return Err(KruonError::Conflict(
                "an active run must be cancelled or completed before a follow-up can start".into(),
            ));
        }
        let entry = self.enqueue_task_run(EnqueueTaskRunRequest {
            task_id: task.task_id.clone(),
            adapter: run.adapter,
            timeout_ms: Some(60_000),
        })?;
        self.m2.record_control_event(
            "run",
            run_id,
            "run.follow_up_queued",
            serde_json::json!({"queue_id": entry.queue_id, "task_id": task.task_id}),
        )?;
        Ok(entry)
    }

    pub fn recovery_advice(&self, run_id: &str) -> KruonResult<Vec<RecoveryAdvice>> {
        Ok(recovery_advice(&self.store.get_run(run_id)?))
    }

    pub fn pause_capability(&self) -> PauseCapability {
        pause_capability()
    }

    fn task_workspace_run(
        &self,
        task_id: &str,
        run_id: &str,
    ) -> KruonResult<(TaskRecord, WorkspaceRecord, RunSnapshot)> {
        let task = self.control.get_task_for_run(run_id)?;
        if task.task_id != task_id {
            return Err(KruonError::Conflict(
                "task does not own the supplied run".into(),
            ));
        }
        let workspace = self.control.get_workspace(&task.workspace_id)?;
        let run = self.store.get_run(run_id)?;
        Ok((task, workspace, run))
    }

    fn task_workspace_run_for_run(
        &self,
        run_id: &str,
    ) -> KruonResult<(TaskRecord, WorkspaceRecord, RunSnapshot)> {
        let task = self.control.get_task_for_run(run_id)?;
        self.task_workspace_run(&task.task_id, run_id)
    }

    fn dispatch_queued_runs(self: &Arc<Self>) -> KruonResult<()> {
        let _dispatch = self
            .queue_dispatch_lock
            .lock()
            .expect("queue dispatch mutex poisoned");
        while self.store.active_run_count()? < MAX_CONCURRENT_RUNS {
            let Some(entry) = self.control.claim_next_queued()? else {
                break;
            };
            let result = (|| {
                let task = self.control.get_task(&entry.task_id)?;
                let workspace = self.control.get_workspace(&task.workspace_id)?;
                if !workspace.trusted {
                    return Err(KruonError::PathPolicy(
                        "workspace trust was removed before launch".into(),
                    ));
                }
                validate_task_scopes(&workspace, &task)?;
                let run_id = uuid::Uuid::new_v4().to_string();
                self.control.mark_queue_started(&entry.queue_id, &run_id)?;
                let request = StartRunRequest {
                    adapter: entry.adapter,
                    workspace_root: workspace.root.clone(),
                    working_directory: workspace.root,
                    prompt: task_prompt(&task),
                    timeout_ms: entry.timeout_ms,
                    policy_id: Some(format!("workspace:{}:read_only", task.workspace_id)),
                };
                self.start_with_run_id(request, run_id).map(|_| ())
            })();
            if let Err(error) = result {
                self.control
                    .mark_queue_failed(&entry.queue_id, public_error_code(&error))?;
            }
        }
        Ok(())
    }

    fn append_next(
        &self,
        run_id: &str,
        event_type: &str,
        phase: EventPhase,
        terminal_state: Option<TerminalState>,
        payload: Value,
    ) -> KruonResult<EventEnvelope> {
        let run_lock = {
            let mut locks = self.append_locks.lock().expect("append lock map poisoned");
            Arc::clone(
                locks
                    .entry(run_id.to_owned())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        let _guard = run_lock.lock().expect("run append mutex poisoned");
        let snapshot = self.store.get_run(run_id)?;
        if snapshot.terminal_state.is_some() {
            return Err(KruonError::Conflict(format!(
                "run {run_id} is already terminal"
            )));
        }
        let event = EventEnvelope::new(
            run_id,
            snapshot.last_sequence + 1,
            event_type,
            phase,
            terminal_state,
            payload,
        );
        self.store.append_event(&event)?;
        Ok(event)
    }

    fn finalize_outcome(
        &self,
        run_id: &str,
        adapter: super::domain::AdapterKind,
        outcome: ProcessOutcome,
    ) -> KruonResult<()> {
        let finalize_lock = {
            let mut locks = self
                .finalize_locks
                .lock()
                .expect("finalize lock map poisoned");
            Arc::clone(
                locks
                    .entry(run_id.to_owned())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        let _guard = finalize_lock.lock().expect("run finalize mutex poisoned");
        if self.store.get_run(run_id)?.terminal_state.is_some() {
            return Ok(());
        }

        (|| {
            if outcome.forced_stop_required {
                self.append_next(
                    run_id,
                    "run.forced_stop_required",
                    EventPhase::Cancelling,
                    None,
                    serde_json::json!({"reason": outcome.reason}),
                )?;
            }
            for line in outcome
                .stdout_lines
                .iter()
                .chain(outcome.stderr_lines.iter())
            {
                let mut event = self.adapter_host.normalize_line(adapter, run_id, 0, line);
                if let Some(source_terminal) = event.terminal_state.take() {
                    event.phase = EventPhase::Running;
                    if let Some(object) = event.payload.as_object_mut() {
                        object.insert(
                            "source_terminal_state".into(),
                            serde_json::to_value(source_terminal)?,
                        );
                    }
                }
                self.append_normalized(run_id, event)?;
            }
            self.append_next(
                run_id,
                "run.terminal",
                EventPhase::Terminal,
                Some(outcome.terminal_state),
                serde_json::json!({
                    "status": outcome.status,
                    "terminal_state": outcome.terminal_state,
                    "exit_code": outcome.exit_code,
                    "forced_stop_required": outcome.forced_stop_required,
                    "residual_detected": outcome.residual_detected,
                    "reason": outcome.reason,
                    "stdout_line_count": outcome.stdout_lines.len(),
                    "stderr_line_count": outcome.stderr_lines.len(),
                    "stdout_truncated": outcome.stdout_truncated,
                    "stderr_truncated": outcome.stderr_truncated,
                    "stdout_lossy_lines": outcome.stdout_lossy_lines,
                    "stderr_lossy_lines": outcome.stderr_lossy_lines,
                }),
            )?;
            Ok(())
        })()
    }

    fn append_normalized(&self, run_id: &str, mut event: EventEnvelope) -> KruonResult<()> {
        let run_lock = {
            let mut locks = self.append_locks.lock().expect("append lock map poisoned");
            Arc::clone(
                locks
                    .entry(run_id.to_owned())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        let _guard = run_lock.lock().expect("run append mutex poisoned");
        let snapshot = self.store.get_run(run_id)?;
        if snapshot.terminal_state.is_some() {
            return Ok(());
        }
        event.sequence = snapshot.last_sequence + 1;
        self.store.append_event(&event)?;
        Ok(())
    }
}

fn values_for_keys(value: &Value, keys: &[&str]) -> Vec<String> {
    let mut values = Vec::new();
    collect_values_for_keys(value, keys, &mut values);
    values.sort();
    values.dedup();
    values
}

fn collect_values_for_keys(value: &Value, keys: &[&str], values: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if keys
                    .iter()
                    .any(|candidate| key.eq_ignore_ascii_case(candidate))
                {
                    if let Some(value) = value.as_str() {
                        values.push(value.to_owned());
                    }
                }
                collect_values_for_keys(value, keys, values);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_values_for_keys(item, keys, values);
            }
        }
        _ => {}
    }
}

fn safe_workspace_file(
    workspace: &WorkspaceRecord,
    task: Option<&TaskRecord>,
    raw_path: &str,
) -> KruonResult<Option<(String, Option<String>, u64)>> {
    let root = PathBuf::from(&workspace.root);
    let candidate = Path::new(raw_path);
    let candidate = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let Ok(canonical) = candidate.canonicalize() else {
        return Ok(None);
    };
    if !canonical.starts_with(&root) || !canonical.is_file() {
        return Ok(None);
    }
    if let Some(task) = task {
        let scope_roots = canonical_scope_roots(workspace, task)?;
        if !scope_roots.iter().any(|scope| canonical.starts_with(scope)) {
            return Ok(None);
        }
    }
    let relative = canonical
        .strip_prefix(&root)
        .map_err(|_| KruonError::PathPolicy("artifact path escaped workspace".into()))?
        .to_string_lossy()
        .replace('\\', "/");
    let byte_length = std::fs::metadata(&canonical)?.len();
    // Content is deliberately not reread: the normalized adapter event already
    // supplies evidence, while avoiding a symlink-swap content read during review.
    Ok(Some((relative, None, byte_length)))
}

fn validate_report_paths(
    workspace: &WorkspaceRecord,
    task: &TaskRecord,
    paths: &[String],
) -> KruonResult<()> {
    let root = PathBuf::from(&workspace.root);
    let scopes = canonical_scope_roots(workspace, task)?;
    for path in paths {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            continue;
        }
        let relative = Path::new(trimmed);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| component == Component::ParentDir)
        {
            return Err(KruonError::PathPolicy(
                "completion report path is outside the task workspace".into(),
            ));
        }
        let candidate = root.join(relative);
        let parent = candidate
            .parent()
            .unwrap_or(&root)
            .canonicalize()
            .map_err(|_| {
                KruonError::PathPolicy(
                    "completion report path parent must exist in the workspace".into(),
                )
            })?;
        if !parent.starts_with(&root) || !scopes.iter().any(|scope| parent.starts_with(scope)) {
            return Err(KruonError::PathPolicy(
                "completion report path is outside the task scope".into(),
            ));
        }
    }
    Ok(())
}

fn canonical_scope_roots(
    workspace: &WorkspaceRecord,
    task: &TaskRecord,
) -> KruonResult<Vec<PathBuf>> {
    let root = PathBuf::from(&workspace.root);
    task.allowed_paths
        .iter()
        .map(|scope| {
            let candidate = root.join(scope);
            let canonical = candidate.canonicalize().map_err(|_| {
                KruonError::PathPolicy(
                    "task scope must resolve inside the trusted workspace".into(),
                )
            })?;
            if !canonical.starts_with(&root) {
                return Err(KruonError::PathPolicy(
                    "task scope is outside the trusted workspace".into(),
                ));
            }
            Ok(canonical)
        })
        .collect()
}

#[tauri::command]
pub fn start_run(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    request: StartRunRequest,
) -> Result<RunSnapshot, String> {
    state.inner().start(request).map_err(public_error)
}

#[tauri::command]
pub fn cancel_run(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    run_id: String,
) -> Result<RunSnapshot, String> {
    state.cancel(&run_id).map_err(public_error)
}

#[tauri::command]
pub fn get_run(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    run_id: String,
) -> Result<RunSnapshot, String> {
    state.get_run(&run_id).map_err(public_error)
}

#[tauri::command]
pub fn list_events(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    run_id: String,
    after_sequence: Option<u64>,
) -> Result<Vec<EventEnvelope>, String> {
    state
        .list_events(&run_id, after_sequence.unwrap_or(0))
        .map_err(public_error)
}

#[tauri::command]
pub fn replay_run(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    run_id: String,
) -> Result<ReplayResult, String> {
    state.replay_run(&run_id).map_err(public_error)
}

#[tauri::command]
pub fn probe_connections() -> Vec<AdapterConnection> {
    discover_connections()
}

#[tauri::command]
pub fn create_workspace(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    request: WorkspaceCreateRequest,
) -> Result<WorkspaceRecord, String> {
    state.create_workspace(request).map_err(public_error)
}

#[tauri::command]
pub fn list_workspaces(
    state: tauri::State<'_, Arc<RuntimeCore>>,
) -> Result<Vec<WorkspaceRecord>, String> {
    state.list_workspaces().map_err(public_error)
}

#[tauri::command]
pub fn trust_workspace(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    workspace_id: String,
) -> Result<WorkspaceRecord, String> {
    state.trust_workspace(&workspace_id).map_err(public_error)
}

#[tauri::command]
pub fn untrust_workspace(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    workspace_id: String,
) -> Result<WorkspaceRecord, String> {
    state.untrust_workspace(&workspace_id).map_err(public_error)
}

#[tauri::command]
pub fn create_task(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    request: TaskCreateRequest,
) -> Result<TaskRecord, String> {
    state.create_task(request).map_err(public_error)
}

#[tauri::command]
pub fn create_sample_task(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    workspace_id: String,
) -> Result<TaskRecord, String> {
    state
        .create_sample_task(&workspace_id)
        .map_err(public_error)
}

#[tauri::command]
pub fn list_tasks(state: tauri::State<'_, Arc<RuntimeCore>>) -> Result<Vec<TaskRecord>, String> {
    state.list_tasks().map_err(public_error)
}

#[tauri::command]
pub fn enqueue_task_run(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    request: EnqueueTaskRunRequest,
) -> Result<QueueEntry, String> {
    state
        .inner()
        .enqueue_task_run(request)
        .map_err(public_error)
}

#[tauri::command]
pub fn list_queue(state: tauri::State<'_, Arc<RuntimeCore>>) -> Result<Vec<QueueEntry>, String> {
    state.list_queue().map_err(public_error)
}

#[tauri::command]
pub fn list_approvals(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    run_id: Option<String>,
) -> Result<Vec<super::m2::ApprovalRecord>, String> {
    state
        .list_approvals(run_id.as_deref())
        .map_err(public_error)
}

#[tauri::command]
pub fn list_artifacts(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    run_id: String,
) -> Result<Vec<ArtifactRecord>, String> {
    state.list_artifacts(&run_id).map_err(public_error)
}

#[tauri::command]
pub fn collect_artifacts(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    run_id: String,
) -> Result<Vec<ArtifactRecord>, String> {
    state.collect_artifacts(&run_id).map_err(public_error)
}

#[tauri::command]
pub fn record_completion_report(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    report: CompletionReportCreate,
) -> Result<ArtifactRecord, String> {
    state.record_completion_report(report).map_err(public_error)
}

#[tauri::command]
pub fn latest_task_reviews(
    state: tauri::State<'_, Arc<RuntimeCore>>,
) -> Result<Vec<TaskReviewRecord>, String> {
    state.latest_task_reviews().map_err(public_error)
}

#[tauri::command]
pub fn list_run_audit(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    run_id: String,
) -> Result<Vec<super::m2::AuditRecord>, String> {
    state.list_run_audit(&run_id).map_err(public_error)
}

#[tauri::command]
pub fn accept_task(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    task_id: String,
    run_id: String,
    note: String,
) -> Result<TaskReviewRecord, String> {
    state
        .accept_task(&task_id, &run_id, &note)
        .map_err(public_error)
}

#[tauri::command]
pub fn return_task(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    task_id: String,
    run_id: String,
    note: String,
) -> Result<TaskReviewRecord, String> {
    state
        .return_task(&task_id, &run_id, &note)
        .map_err(public_error)
}

#[tauri::command]
pub fn restart_follow_up(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    run_id: String,
) -> Result<QueueEntry, String> {
    state
        .inner()
        .restart_follow_up(&run_id)
        .map_err(public_error)
}

#[tauri::command]
pub fn get_recovery_advice(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    run_id: String,
) -> Result<Vec<RecoveryAdvice>, String> {
    state.recovery_advice(&run_id).map_err(public_error)
}

#[tauri::command]
pub fn get_pause_capability(state: tauri::State<'_, Arc<RuntimeCore>>) -> PauseCapability {
    state.pause_capability()
}

#[tauri::command]
pub fn list_runs(state: tauri::State<'_, Arc<RuntimeCore>>) -> Result<Vec<RunSnapshot>, String> {
    state.list_runs().map_err(public_error)
}

fn public_error(error: KruonError) -> String {
    let code = public_error_code(&error);
    let message = match error {
        KruonError::NotFound(_) => "requested local record was not found",
        KruonError::Conflict(_) => "request conflicts with current run state",
        KruonError::PathPolicy(_) => {
            "workspace is not trusted or a path is outside its allowed scope"
        }
        KruonError::InvalidArgument(_) => "request is invalid",
        KruonError::Process(_) => "local process operation failed",
        KruonError::Adapter(_) => "adapter operation failed",
        KruonError::Compatibility(_) => {
            "installed adapter version is outside the Alpha compatibility matrix"
        }
        KruonError::Store(_) | KruonError::Serialization(_) => "local event store operation failed",
        KruonError::Io(_) => "local I/O operation failed",
    };
    format!("{code}: {message}")
}

fn public_error_code(error: &KruonError) -> &'static str {
    match error {
        KruonError::NotFound(_) => "not_found",
        KruonError::Conflict(_) => "conflict",
        KruonError::PathPolicy(_) => "path_policy_violation",
        KruonError::InvalidArgument(_) => "invalid_argument",
        KruonError::Process(_) => "process_error",
        KruonError::Adapter(_) => "adapter_error",
        KruonError::Compatibility(_) => "unsupported_adapter_version",
        KruonError::Store(_) | KruonError::Serialization(_) => "store_error",
        KruonError::Io(_) => "internal_error",
    }
}

#[cfg(test)]
mod m1_runtime_tests {
    use super::*;
    use crate::core::domain::AdapterKind;

    #[test]
    fn onboarding_sample_requires_trust_and_is_idempotent() {
        let root = tempfile::tempdir().unwrap();
        let runtime = Arc::new(RuntimeCore::new(
            Arc::new(EventStore::open_in_memory().unwrap()),
            Arc::new(ProcessSupervisor::default()),
        ));
        let workspace = runtime
            .create_workspace(WorkspaceCreateRequest {
                root: root.path().to_string_lossy().into_owned(),
                display_name: "Onboarding fixture".into(),
            })
            .unwrap();

        assert!(matches!(
            runtime.create_sample_task(&workspace.workspace_id),
            Err(KruonError::PathPolicy(_))
        ));

        runtime.trust_workspace(&workspace.workspace_id).unwrap();
        let first = runtime.create_sample_task(&workspace.workspace_id).unwrap();
        let second = runtime.create_sample_task(&workspace.workspace_id).unwrap();

        assert_eq!(first.task_id, second.task_id);
        assert_eq!(first.title, ONBOARDING_SAMPLE_TASK_TITLE);
        assert_eq!(first.context, ONBOARDING_SAMPLE_TASK_CONTEXT);
        assert_eq!(first.allowed_paths, vec!["."]);
        assert!(first.goal.contains("without changing files"));
        assert_eq!(runtime.list_tasks().unwrap().len(), 1);
    }

    #[test]
    fn revoked_workspace_trust_blocks_new_queue_entries() {
        let root = tempfile::tempdir().unwrap();
        let runtime = Arc::new(RuntimeCore::new(
            Arc::new(EventStore::open_in_memory().unwrap()),
            Arc::new(ProcessSupervisor::default()),
        ));
        let workspace = runtime
            .create_workspace(WorkspaceCreateRequest {
                root: root.path().to_string_lossy().into_owned(),
                display_name: "Revocation fixture".into(),
            })
            .unwrap();
        runtime.trust_workspace(&workspace.workspace_id).unwrap();
        let task = runtime
            .create_task(TaskCreateRequest {
                workspace_id: workspace.workspace_id.clone(),
                title: "Do not launch".into(),
                goal: "Prove revoked trust blocks launch".into(),
                context: "Security regression".into(),
                allowed_paths: vec![".".into()],
                acceptance_criteria: "No queue entry is created".into(),
                test_plan: "Revoke trust before enqueue".into(),
                rollback_plan: "No changes".into(),
            })
            .unwrap();

        let revoked = runtime.untrust_workspace(&workspace.workspace_id).unwrap();
        assert!(!revoked.trusted);
        assert!(matches!(
            runtime.enqueue_task_run(EnqueueTaskRunRequest {
                task_id: task.task_id,
                adapter: AdapterKind::Codex,
                timeout_ms: None,
            }),
            Err(KruonError::PathPolicy(_))
        ));
        assert!(runtime.list_queue().unwrap().is_empty());
    }

    #[test]
    fn untrusted_workspace_cannot_enqueue_a_noninteractive_cli_run() {
        let root = tempfile::tempdir().unwrap();
        let runtime = Arc::new(RuntimeCore::new(
            Arc::new(EventStore::open_in_memory().unwrap()),
            Arc::new(ProcessSupervisor::default()),
        ));
        let workspace = runtime
            .control
            .create_workspace(WorkspaceCreateRequest {
                root: root.path().to_string_lossy().into_owned(),
                display_name: "Untrusted fixture".into(),
            })
            .unwrap();
        let task = runtime
            .control
            .create_task(TaskCreateRequest {
                workspace_id: workspace.workspace_id,
                title: "Blocked launch".into(),
                goal: "Prove the trust gate".into(),
                context: "".into(),
                allowed_paths: vec![".".into()],
                acceptance_criteria: "No process is started".into(),
                test_plan: "Attempt to queue the task".into(),
                rollback_plan: "No changes".into(),
            })
            .unwrap();

        assert!(matches!(
            runtime.enqueue_task_run(EnqueueTaskRunRequest {
                task_id: task.task_id,
                adapter: AdapterKind::Codex,
                timeout_ms: None,
            }),
            Err(KruonError::PathPolicy(_))
        ));
        assert!(runtime.list_queue().unwrap().is_empty());
        assert!(runtime.list_runs().unwrap().is_empty());
    }

    #[test]
    fn runtime_open_applies_the_complete_transactional_schema_set() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("runtime.sqlite3");
        let runtime = RuntimeCore::open(&database).unwrap();
        drop(runtime);

        let connection = rusqlite::Connection::open(database).unwrap();
        let mut statement = connection
            .prepare("SELECT version FROM schema_migrations ORDER BY version")
            .unwrap();
        let versions = statement
            .query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(versions, vec![1, 2, 3, 4]);
    }
}

#[cfg(test)]
mod m2_runtime_tests {
    use super::*;
    use crate::core::domain::{AdapterKind, EventEnvelope, EventPhase};
    use crate::core::m2::CompletionReportCreate;

    fn completed_task_run() -> (Arc<RuntimeCore>, TaskRecord, tempfile::TempDir, String) {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("allowed")).unwrap();
        std::fs::write(root.path().join("allowed").join("inside.txt"), "fixture").unwrap();
        let runtime = Arc::new(RuntimeCore::new(
            Arc::new(EventStore::open_in_memory().unwrap()),
            Arc::new(ProcessSupervisor::default()),
        ));
        let workspace = runtime
            .control
            .create_workspace(WorkspaceCreateRequest {
                root: root.path().to_string_lossy().into_owned(),
                display_name: "M2 fixture".into(),
            })
            .unwrap();
        runtime
            .control
            .set_workspace_trust(&workspace.workspace_id, true)
            .unwrap();
        let task = runtime
            .control
            .create_task(TaskCreateRequest {
                workspace_id: workspace.workspace_id,
                title: "Review controlled completion".into(),
                goal: "Verify the manual M2 handoff".into(),
                context: "fixture".into(),
                allowed_paths: vec!["allowed".into()],
                acceptance_criteria: "A human accepts only after the report".into(),
                test_plan: "Record the completion report".into(),
                rollback_plan: "No changes".into(),
            })
            .unwrap();
        let entry = runtime
            .control
            .enqueue(EnqueueTaskRunRequest {
                task_id: task.task_id.clone(),
                adapter: AdapterKind::Codex,
                timeout_ms: None,
            })
            .unwrap();
        let run_id = "m2-completed-run".to_owned();
        runtime
            .control
            .mark_queue_started(&entry.queue_id, &run_id)
            .unwrap();
        let request = StartRunRequest {
            adapter: AdapterKind::Codex,
            workspace_root: root.path().to_string_lossy().into_owned(),
            working_directory: root.path().to_string_lossy().into_owned(),
            prompt: "read-only fixture".into(),
            timeout_ms: None,
            policy_id: Some("test-read-only".into()),
        };
        runtime
            .store
            .create_run(&run_id, &request, root.path(), root.path(), "frozen-plan")
            .unwrap();
        runtime
            .store
            .append_event(&EventEnvelope::new(
                &run_id,
                1,
                "run.terminal",
                EventPhase::Terminal,
                Some(TerminalState::Completed),
                serde_json::json!({"exit_code": 0}),
            ))
            .unwrap();
        (runtime, task, root, run_id)
    }

    #[test]
    fn task_acceptance_requires_a_completed_run_and_completion_report() {
        let (runtime, task, _root, run_id) = completed_task_run();
        assert!(matches!(
            runtime.accept_task(&task.task_id, &run_id, ""),
            Err(KruonError::Conflict(_))
        ));
        runtime
            .record_completion_report(CompletionReportCreate {
                run_id: run_id.clone(),
                task_id: task.task_id.clone(),
                summary: "Human-readable test handoff".into(),
                tests: vec![],
                changed_paths: vec!["allowed/inside.txt".into()],
            })
            .unwrap();
        let review = runtime.accept_task(&task.task_id, &run_id, "").unwrap();
        assert_eq!(review.status, TaskReviewStatus::Accepted);
        assert_eq!(review.note, "Accepted after artifact review");
    }

    #[test]
    fn artifacts_and_reports_cannot_escape_the_task_scope() {
        let (runtime, task, root, run_id) = completed_task_run();
        let workspace = runtime.control.get_workspace(&task.workspace_id).unwrap();
        let inside = root.path().join("allowed").join("inside.txt");
        let outside = root.path().join("outside.txt");
        std::fs::write(&outside, "outside").unwrap();
        assert!(
            safe_workspace_file(&workspace, Some(&task), &inside.to_string_lossy())
                .unwrap()
                .is_some()
        );
        assert!(
            safe_workspace_file(&workspace, Some(&task), &outside.to_string_lossy())
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            runtime.record_completion_report(CompletionReportCreate {
                run_id,
                task_id: task.task_id,
                summary: "Attempted out-of-scope report".into(),
                tests: vec![],
                changed_paths: vec!["outside.txt".into()],
            }),
            Err(KruonError::PathPolicy(_))
        ));
    }
}

#[cfg(test)]
mod m4_fault_tests {
    use super::*;
    use crate::core::domain::{AdapterKind, RunStatus};

    fn runtime_with_pending_run(run_id: &str) -> (RuntimeCore, tempfile::TempDir) {
        let workspace = tempfile::tempdir().unwrap();
        let store = Arc::new(EventStore::open_in_memory().unwrap());
        let runtime = RuntimeCore::new(store.clone(), Arc::new(ProcessSupervisor::default()));
        let request = StartRunRequest {
            adapter: AdapterKind::Codex,
            workspace_root: workspace.path().to_string_lossy().into_owned(),
            working_directory: workspace.path().to_string_lossy().into_owned(),
            prompt: "fault fixture".into(),
            timeout_ms: Some(1_000),
            policy_id: Some("fault-injection".into()),
        };
        store
            .create_run(
                run_id,
                &request,
                workspace.path(),
                workspace.path(),
                "fault-fingerprint",
            )
            .unwrap();
        (runtime, workspace)
    }

    #[test]
    fn crash_and_timeout_outcomes_never_become_completed() {
        let (crash_runtime, _crash_workspace) = runtime_with_pending_run("crash");
        crash_runtime
            .finalize_outcome(
                "crash",
                AdapterKind::Codex,
                ProcessOutcome {
                    status: RunStatus::Failed,
                    terminal_state: TerminalState::Failed,
                    exit_code: Some(7),
                    forced_stop_required: false,
                    residual_detected: false,
                    reason: "process exited with code 7".into(),
                    stdout_lines: vec!["{\"type\":\"thread.started\"}".into()],
                    stderr_lines: vec!["fatal fixture".into()],
                    stdout_truncated: false,
                    stderr_truncated: false,
                    stdout_lossy_lines: 0,
                    stderr_lossy_lines: 0,
                },
            )
            .unwrap();
        let crash = crash_runtime.get_run("crash").unwrap();
        assert_eq!(crash.status, RunStatus::Failed);
        assert_eq!(crash.terminal_state, Some(TerminalState::Failed));

        let (timeout_runtime, _timeout_workspace) = runtime_with_pending_run("timeout");
        timeout_runtime
            .finalize_outcome(
                "timeout",
                AdapterKind::Codex,
                ProcessOutcome {
                    status: RunStatus::Cancelled,
                    terminal_state: TerminalState::Cancelled,
                    exit_code: None,
                    forced_stop_required: true,
                    residual_detected: false,
                    reason: "timeout".into(),
                    stdout_lines: Vec::new(),
                    stderr_lines: Vec::new(),
                    stdout_truncated: false,
                    stderr_truncated: false,
                    stdout_lossy_lines: 0,
                    stderr_lossy_lines: 0,
                },
            )
            .unwrap();
        let timeout = timeout_runtime.get_run("timeout").unwrap();
        assert_eq!(timeout.status, RunStatus::Cancelled);
        assert_eq!(timeout.terminal_state, Some(TerminalState::Cancelled));
    }

    #[test]
    fn terminal_diagnostics_preserve_bounded_output_flags() {
        let (runtime, _workspace) = runtime_with_pending_run("bounded-output");
        runtime
            .finalize_outcome(
                "bounded-output",
                AdapterKind::Codex,
                ProcessOutcome {
                    status: RunStatus::Failed,
                    terminal_state: TerminalState::Failed,
                    exit_code: Some(1),
                    forced_stop_required: false,
                    residual_detected: false,
                    reason: "malformed output fixture".into(),
                    stdout_lines: vec!["invalid \u{fffd}".into()],
                    stderr_lines: Vec::new(),
                    stdout_truncated: true,
                    stderr_truncated: false,
                    stdout_lossy_lines: 1,
                    stderr_lossy_lines: 0,
                },
            )
            .unwrap();
        let events = runtime.list_events("bounded-output", 0).unwrap();
        let terminal = events.last().unwrap();
        assert_eq!(terminal.event_type, "run.terminal");
        assert_eq!(terminal.payload["stdout_truncated"], true);
        assert_eq!(terminal.payload["stdout_lossy_lines"], 1);
        assert_eq!(terminal.terminal_state, Some(TerminalState::Failed));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::OnceLock;
    use std::time::Instant;

    use super::*;
    use crate::core::domain::{AdapterKind, RunStatus};
    use crate::core::m1::QueueState;

    #[test]
    fn public_errors_do_not_expose_paths_or_internal_details() {
        let secret_path = "/Users/example/private/project";
        let error = public_error(KruonError::PathPolicy(secret_path.into()));
        assert_eq!(
            error,
            "path_policy_violation: workspace is not trusted or a path is outside its allowed scope"
        );
        assert!(!error.contains(secret_path));

        let store_error = public_error(KruonError::Store("database /secret/db is corrupt".into()));
        assert_eq!(
            store_error,
            "store_error: local event store operation failed"
        );
        assert!(!store_error.contains("/secret/db"));
    }

    fn environment_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn install_fake_cli_with_version(directory: &Path, name: &str, version: &str, script: &str) {
        let path = directory.join(name);
        let probe = match name {
            "codex" => format!(
                "if [ \"$1\" = \"exec\" ] && [ \"$2\" = \"--version\" ]; then echo 'codex-cli {version}'; exit 0; fi\nif [ \"$1\" = \"login\" ] && [ \"$2\" = \"status\" ]; then echo 'Logged in'; exit 0; fi"
            ),
            "claude" => format!(
                "if [ \"$1\" = \"--version\" ]; then echo '{version} (Claude Code)'; exit 0; fi\nif [ \"$1\" = \"auth\" ] && [ \"$2\" = \"status\" ]; then echo 'Authenticated'; exit 0; fi"
            ),
            _ => panic!("unknown fixture adapter"),
        };
        std::fs::write(&path, format!("#!/bin/sh\n{probe}\n{script}\n")).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    fn install_fake_cli(directory: &Path, name: &str, script: &str) {
        let version = match name {
            "codex" => "0.144.1",
            "claude" => "2.1.211",
            _ => panic!("unknown fixture adapter"),
        };
        install_fake_cli_with_version(directory, name, version, script);
    }

    fn prepend_path(directory: &Path) -> std::ffi::OsString {
        let previous = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![directory.to_path_buf()];
        paths.extend(std::env::split_paths(&previous));
        unsafe { std::env::set_var("PATH", std::env::join_paths(paths).unwrap()) };
        previous
    }

    fn restore_path(previous: std::ffi::OsString) {
        unsafe { std::env::set_var("PATH", previous) };
    }

    fn wait_terminal(runtime: &RuntimeCore, run_id: &str) -> RunSnapshot {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let snapshot = runtime.get_run(run_id).unwrap();
            if snapshot.terminal_state.is_some() {
                return snapshot;
            }
            assert!(Instant::now() < deadline, "run did not become terminal");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn fixed_plans_persist_and_replay_one_terminal_event_per_adapter() {
        let _environment = environment_lock().lock().unwrap();
        let bin = tempfile::tempdir().unwrap();
        install_fake_cli(
            bin.path(),
            "codex",
            "cat >/dev/null; printf '%s\\n' '{\"type\":\"thread.started\"}' '{\"type\":\"turn.completed\"}'",
        );
        install_fake_cli(
            bin.path(),
            "claude",
            "cat >/dev/null; printf '%s\\n' '{\"type\":\"system\"}' '{\"type\":\"result\",\"is_error\":false}'",
        );
        let previous_path = prepend_path(bin.path());
        let workspace = tempfile::tempdir().unwrap();
        let runtime = Arc::new(RuntimeCore::new(
            Arc::new(EventStore::open_in_memory().unwrap()),
            Arc::new(ProcessSupervisor::new(
                Duration::from_millis(100),
                Duration::from_secs(1),
            )),
        ));

        for adapter in [AdapterKind::Codex, AdapterKind::Claude] {
            let snapshot = runtime
                .start(StartRunRequest {
                    adapter,
                    workspace_root: workspace.path().to_string_lossy().into_owned(),
                    working_directory: workspace.path().to_string_lossy().into_owned(),
                    prompt: "read-only synthetic prompt".into(),
                    timeout_ms: Some(2_000),
                    policy_id: Some("test".into()),
                })
                .unwrap();
            let terminal = wait_terminal(&runtime, &snapshot.run_id);
            assert_eq!(terminal.status, RunStatus::Completed);
            let replay = runtime.replay_run(&snapshot.run_id).unwrap();
            assert_eq!(
                replay
                    .events
                    .iter()
                    .filter(|event| event.terminal_state.is_some())
                    .count(),
                1
            );
            assert!(replay
                .events
                .iter()
                .any(|event| event.event_type.starts_with(adapter.as_str())));
        }
        restore_path(previous_path);
    }

    #[test]
    fn runtime_blocks_an_installed_version_outside_the_alpha_matrix() {
        let _environment = environment_lock().lock().unwrap();
        let bin = tempfile::tempdir().unwrap();
        install_fake_cli_with_version(
            bin.path(),
            "codex",
            "9.9.9",
            "cat >/dev/null; printf '%s\\n' '{\"type\":\"turn.completed\"}'",
        );
        let previous_path = prepend_path(bin.path());
        let workspace = tempfile::tempdir().unwrap();
        let runtime = Arc::new(RuntimeCore::new(
            Arc::new(EventStore::open_in_memory().unwrap()),
            Arc::new(ProcessSupervisor::default()),
        ));

        let result = runtime.start(StartRunRequest {
            adapter: AdapterKind::Codex,
            workspace_root: workspace.path().to_string_lossy().into_owned(),
            working_directory: workspace.path().to_string_lossy().into_owned(),
            prompt: "must not launch".into(),
            timeout_ms: Some(2_000),
            policy_id: Some("test".into()),
        });
        assert!(matches!(result, Err(KruonError::Compatibility(_))));
        assert!(runtime.list_runs().unwrap().is_empty());
        restore_path(previous_path);
    }

    #[test]
    fn trusted_tasks_use_a_durable_two_slot_queue_without_run_mixing() {
        let _environment = environment_lock().lock().unwrap();
        let bin = tempfile::tempdir().unwrap();
        install_fake_cli(
            bin.path(),
            "codex",
            "cat >/dev/null; sleep 1; printf '%s\\n' '{\"type\":\"turn.completed\"}'",
        );
        let previous_path = prepend_path(bin.path());
        let workspace_root = tempfile::tempdir().unwrap();
        let runtime = Arc::new(RuntimeCore::new(
            Arc::new(EventStore::open_in_memory().unwrap()),
            Arc::new(ProcessSupervisor::new(
                Duration::from_millis(100),
                Duration::from_secs(2),
            )),
        ));
        let workspace = runtime
            .control
            .create_workspace(WorkspaceCreateRequest {
                root: workspace_root.path().to_string_lossy().into_owned(),
                display_name: "Queue fixture".into(),
            })
            .unwrap();
        runtime
            .control
            .set_workspace_trust(&workspace.workspace_id, true)
            .unwrap();
        let task = runtime
            .control
            .create_task(TaskCreateRequest {
                workspace_id: workspace.workspace_id,
                title: "Read only queue fixture".into(),
                goal: "Inspect the fixture without file changes".into(),
                context: "Queue isolation test".into(),
                allowed_paths: vec![".".into()],
                acceptance_criteria: "One isolated event stream per run".into(),
                test_plan: "Run the synthetic adapter".into(),
                rollback_plan: "No changes".into(),
            })
            .unwrap();

        for _ in 0..3 {
            runtime
                .enqueue_task_run(EnqueueTaskRunRequest {
                    task_id: task.task_id.clone(),
                    adapter: AdapterKind::Codex,
                    timeout_ms: Some(3_000),
                })
                .unwrap();
        }
        assert_eq!(runtime.store.active_run_count().unwrap(), 2);
        let queue = runtime.list_queue().unwrap();
        assert_eq!(
            queue
                .iter()
                .filter(|entry| entry.state == QueueState::Started)
                .count(),
            2
        );
        assert_eq!(
            queue
                .iter()
                .filter(|entry| entry.state == QueueState::Queued)
                .count(),
            1
        );

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let runs = runtime.list_runs().unwrap();
            if runs.len() == 3 && runs.iter().all(|run| run.terminal_state.is_some()) {
                break;
            }
            assert!(Instant::now() < deadline, "queued runs did not complete");
            assert!(runtime.store.active_run_count().unwrap() <= MAX_CONCURRENT_RUNS);
            std::thread::sleep(Duration::from_millis(20));
        }
        for run in runtime.list_runs().unwrap() {
            let events = runtime.list_events(&run.run_id, 0).unwrap();
            assert!(events.iter().all(|event| event.run_id == run.run_id));
        }
        restore_path(previous_path);
    }

    #[test]
    fn runtime_cancel_records_one_cancelled_terminal() {
        let _environment = environment_lock().lock().unwrap();
        let bin = tempfile::tempdir().unwrap();
        install_fake_cli(
            bin.path(),
            "codex",
            "trap '' TERM; (trap '' TERM; sleep 30) & wait",
        );
        let previous_path = prepend_path(bin.path());
        let workspace = tempfile::tempdir().unwrap();
        let runtime = Arc::new(RuntimeCore::new(
            Arc::new(EventStore::open_in_memory().unwrap()),
            Arc::new(ProcessSupervisor::new(
                Duration::from_millis(100),
                Duration::from_secs(2),
            )),
        ));
        let run = runtime
            .start(StartRunRequest {
                adapter: AdapterKind::Codex,
                workspace_root: workspace.path().to_string_lossy().into_owned(),
                working_directory: workspace.path().to_string_lossy().into_owned(),
                prompt: "cancel me".into(),
                timeout_ms: Some(5_000),
                policy_id: Some("test".into()),
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(500));
        let terminal = runtime.cancel(&run.run_id).unwrap();
        assert_eq!(terminal.status, RunStatus::Cancelled);
        let events = runtime.list_events(&run.run_id, 0).unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.terminal_state.is_some())
                .count(),
            1
        );
        restore_path(previous_path);
    }

    #[test]
    fn forced_outcome_records_transitional_status_before_terminal() {
        let workspace = tempfile::tempdir().unwrap();
        let store = Arc::new(EventStore::open_in_memory().unwrap());
        let runtime = RuntimeCore::new(
            Arc::clone(&store),
            Arc::new(ProcessSupervisor::new(
                Duration::from_millis(100),
                Duration::from_secs(1),
            )),
        );
        let request = StartRunRequest {
            adapter: AdapterKind::Codex,
            workspace_root: workspace.path().to_string_lossy().into_owned(),
            working_directory: workspace.path().to_string_lossy().into_owned(),
            prompt: "synthetic".into(),
            timeout_ms: Some(1_000),
            policy_id: Some("test".into()),
        };
        store
            .create_run(
                "forced",
                &request,
                workspace.path(),
                workspace.path(),
                "test-launch-fingerprint",
            )
            .unwrap();
        runtime
            .finalize_outcome(
                "forced",
                AdapterKind::Codex,
                ProcessOutcome {
                    status: RunStatus::Cancelled,
                    terminal_state: TerminalState::Cancelled,
                    exit_code: None,
                    forced_stop_required: true,
                    residual_detected: false,
                    reason: "timeout".into(),
                    stdout_lines: Vec::new(),
                    stderr_lines: Vec::new(),
                    stdout_truncated: false,
                    stderr_truncated: false,
                    stdout_lossy_lines: 0,
                    stderr_lossy_lines: 0,
                },
            )
            .unwrap();
        let events = runtime.list_events("forced", 0).unwrap();
        assert_eq!(events[0].event_type, "run.forced_stop_required");
        assert_eq!(events[1].event_type, "run.terminal");
        assert_eq!(
            store.get_run("forced").unwrap().status,
            RunStatus::Cancelled
        );
    }
}
