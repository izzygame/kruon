use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use super::adapter_host::AdapterHost;
use super::domain::{
    EventEnvelope, EventPhase, ReplayResult, RunSnapshot, StartRunRequest, TerminalState,
};
use super::error::{KruonError, KruonResult};
use super::event_store::EventStore;
use super::path_policy::PathPolicy;
use super::process_supervisor::{ProcessOutcome, ProcessSupervisor};

pub struct RuntimeCore {
    store: Arc<EventStore>,
    supervisor: Arc<ProcessSupervisor>,
    adapter_host: AdapterHost,
    append_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    finalize_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl RuntimeCore {
    pub fn open(database_path: impl AsRef<Path>) -> KruonResult<Arc<Self>> {
        let store = Arc::new(EventStore::open(database_path)?);
        let runtime = Arc::new(Self::new(store, Arc::new(ProcessSupervisor::default())));
        runtime.store.recover_interrupted_runs()?;
        Ok(runtime)
    }

    pub fn new(store: Arc<EventStore>, supervisor: Arc<ProcessSupervisor>) -> Self {
        Self {
            store,
            supervisor,
            adapter_host: AdapterHost,
            append_locks: Mutex::new(HashMap::new()),
            finalize_locks: Mutex::new(HashMap::new()),
        }
    }

    pub fn store(&self) -> Arc<EventStore> {
        Arc::clone(&self.store)
    }

    pub fn start(self: &Arc<Self>, request: StartRunRequest) -> KruonResult<RunSnapshot> {
        let paths = PathPolicy::validate(&request.workspace_root, &request.working_directory)?;
        let run_id = uuid::Uuid::new_v4().to_string();
        self.store.create_run(
            &run_id,
            &request,
            &paths.workspace_root,
            &paths.working_directory,
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
            }),
        )?;

        let plan = self.adapter_host.launch_plan(
            request.adapter,
            &paths.working_directory,
            &request.prompt,
        )?;
        let handle = match self.supervisor.spawn(&run_id, plan) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = self.append_next(
                    &run_id,
                    "run.spawn_failed",
                    EventPhase::Terminal,
                    Some(TerminalState::Failed),
                    serde_json::json!({"error": error.to_string()}),
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
            }
        });
        self.store.get_run(&run_id)
    }

    pub fn cancel(&self, run_id: &str) -> KruonResult<RunSnapshot> {
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

#[tauri::command]
pub fn start_run(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    request: StartRunRequest,
) -> Result<RunSnapshot, String> {
    state
        .inner()
        .start(request)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn cancel_run(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    run_id: String,
) -> Result<RunSnapshot, String> {
    state.cancel(&run_id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_run(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    run_id: String,
) -> Result<RunSnapshot, String> {
    state.get_run(&run_id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_events(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    run_id: String,
    after_sequence: Option<u64>,
) -> Result<Vec<EventEnvelope>, String> {
    state
        .list_events(&run_id, after_sequence.unwrap_or(0))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn replay_run(
    state: tauri::State<'_, Arc<RuntimeCore>>,
    run_id: String,
) -> Result<ReplayResult, String> {
    state.replay_run(&run_id).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::OnceLock;
    use std::time::Instant;

    use super::*;
    use crate::core::domain::{AdapterKind, RunStatus};

    fn environment_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn install_fake_cli(directory: &Path, name: &str, script: &str) {
        let path = directory.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(path, permissions).unwrap();
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
            .create_run("forced", &request, workspace.path(), workspace.path())
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
