use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::adapter_host::LaunchPlan;
use super::domain::{RunStatus, TerminalState};
use super::error::{KruonError, KruonResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessHandle {
    pub pid: u32,
    pub pgid: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessOutcome {
    pub status: RunStatus,
    pub terminal_state: TerminalState,
    pub exit_code: Option<i32>,
    pub forced_stop_required: bool,
    pub residual_detected: bool,
    pub reason: String,
    pub stdout_lines: Vec<String>,
    pub stderr_lines: Vec<String>,
}

struct ManagedProcess {
    pid: u32,
    pgid: i32,
    child: Mutex<Child>,
    cancel_requested: AtomicBool,
    outcome: Mutex<Option<ProcessOutcome>>,
    stdout_lines: Arc<Mutex<Vec<String>>>,
    stderr_lines: Arc<Mutex<Vec<String>>>,
    readers: Mutex<Vec<JoinHandle<()>>>,
}

pub struct ProcessSupervisor {
    processes: Mutex<HashMap<String, Arc<ManagedProcess>>>,
    term_grace: Duration,
    kill_grace: Duration,
}

impl Default for ProcessSupervisor {
    fn default() -> Self {
        Self::new(Duration::from_secs(10), Duration::from_secs(2))
    }
}

impl ProcessSupervisor {
    pub fn new(term_grace: Duration, kill_grace: Duration) -> Self {
        Self {
            processes: Mutex::new(HashMap::new()),
            term_grace,
            kill_grace,
        }
    }

    pub fn spawn(&self, run_id: &str, plan: LaunchPlan) -> KruonResult<ProcessHandle> {
        if self
            .processes
            .lock()
            .expect("process map mutex poisoned")
            .contains_key(run_id)
        {
            return Err(KruonError::Conflict(format!(
                "run {run_id} already owns a process"
            )));
        }

        let mut command = Command::new(&plan.program);
        command
            .args(&plan.args)
            .current_dir(&plan.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command.spawn().map_err(|error| {
            KruonError::Process(format!("failed to spawn {}: {error}", plan.program))
        })?;
        let pid = child.id();
        let pgid = i32::try_from(pid)
            .map_err(|_| KruonError::Process(format!("PID {pid} does not fit in i32")))?;

        let stdin = child.stdin.take();

        let stdout_lines = Arc::new(Mutex::new(Vec::new()));
        let stderr_lines = Arc::new(Mutex::new(Vec::new()));
        let mut readers = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            readers.push(drain_lines(stdout, Arc::clone(&stdout_lines)));
        }
        if let Some(stderr) = child.stderr.take() {
            readers.push(drain_lines(stderr, Arc::clone(&stderr_lines)));
        }

        let managed = Arc::new(ManagedProcess {
            pid,
            pgid,
            child: Mutex::new(child),
            cancel_requested: AtomicBool::new(false),
            outcome: Mutex::new(None),
            stdout_lines,
            stderr_lines,
            readers: Mutex::new(readers),
        });
        self.processes
            .lock()
            .expect("process map mutex poisoned")
            .insert(run_id.to_owned(), managed);
        if let Some(mut stdin) = stdin {
            if let Err(error) = stdin.write_all(plan.prompt_stdin.as_bytes()).and_then(|_| {
                if plan.prompt_stdin.ends_with('\n') {
                    Ok(())
                } else {
                    stdin.write_all(b"\n")
                }
            }) {
                let _ = self.cancel(run_id, "stdin_write_failed");
                return Err(error.into());
            }
        }
        Ok(ProcessHandle { pid, pgid })
    }

    pub fn wait(&self, run_id: &str, timeout: Duration) -> KruonResult<ProcessOutcome> {
        let managed = self.get(run_id)?;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(outcome) = managed
                .outcome
                .lock()
                .expect("outcome mutex poisoned")
                .clone()
            {
                return Ok(outcome);
            }
            let child_status = {
                managed
                    .child
                    .lock()
                    .expect("child mutex poisoned")
                    .try_wait()?
            };
            if let Some(status) = child_status {
                return Ok(finalize_natural(&managed, status.code()));
            }
            if Instant::now() >= deadline {
                return self.cancel(run_id, "timeout");
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    pub fn cancel(&self, run_id: &str, reason: &str) -> KruonResult<ProcessOutcome> {
        let managed = self.get(run_id)?;
        if let Some(outcome) = managed
            .outcome
            .lock()
            .expect("outcome mutex poisoned")
            .clone()
        {
            return Ok(outcome);
        }
        managed.cancel_requested.store(true, Ordering::SeqCst);

        let term_result = signal_owned_group(&managed, libc::SIGTERM);
        if let Err(error) = term_result {
            return Ok(finalize_uncertain(
                &managed,
                false,
                true,
                format!("{reason}: SIGTERM failed: {error}"),
            ));
        }
        if wait_for_exit(&managed, self.term_grace)? {
            return Ok(finalize_cancelled(&managed, false, reason));
        }

        if let Err(error) = signal_owned_group(&managed, libc::SIGKILL) {
            return Ok(finalize_uncertain(
                &managed,
                true,
                true,
                format!("{reason}: SIGKILL failed: {error}"),
            ));
        }
        let exited = wait_for_exit(&managed, self.kill_grace)?;
        let residual = group_exists(managed.pgid)?;
        if !exited || residual {
            return Ok(finalize_uncertain(
                &managed,
                true,
                residual,
                format!("{reason}: process-group cleanup could not be confirmed"),
            ));
        }
        Ok(finalize_cancelled(&managed, true, reason))
    }

    pub fn request_cancel(&self, run_id: &str) -> KruonResult<()> {
        self.get(run_id)?
            .cancel_requested
            .store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn handle(&self, run_id: &str) -> KruonResult<ProcessHandle> {
        let managed = self.get(run_id)?;
        Ok(ProcessHandle {
            pid: managed.pid,
            pgid: managed.pgid,
        })
    }

    fn get(&self, run_id: &str) -> KruonResult<Arc<ManagedProcess>> {
        self.processes
            .lock()
            .expect("process map mutex poisoned")
            .get(run_id)
            .cloned()
            .ok_or_else(|| KruonError::NotFound(run_id.to_owned()))
    }
}

fn drain_lines<R: std::io::Read + Send + 'static>(
    reader: R,
    target: Arc<Mutex<Vec<String>>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            target.lock().expect("output mutex poisoned").push(line);
        }
    })
}

fn wait_for_exit(managed: &ManagedProcess, duration: Duration) -> KruonResult<bool> {
    let deadline = Instant::now() + duration;
    loop {
        if managed
            .child
            .lock()
            .expect("child mutex poisoned")
            .try_wait()?
            .is_some()
        {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn signal_owned_group(managed: &ManagedProcess, signal: i32) -> KruonResult<()> {
    if managed.pgid <= 1 || managed.pgid != managed.pid as i32 {
        return Err(KruonError::Process(format!(
            "refusing to signal unowned process group {} for PID {}",
            managed.pgid, managed.pid
        )));
    }
    let result = unsafe { libc::kill(-managed.pgid, signal) };
    if result == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        return Ok(());
    }
    Err(error.into())
}

fn group_exists(pgid: i32) -> KruonResult<bool> {
    if pgid <= 1 {
        return Err(KruonError::Process(format!("invalid process group {pgid}")));
    }
    let result = unsafe { libc::kill(-pgid, 0) };
    if result == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Ok(true),
        _ => Err(error.into()),
    }
}

fn finalize_natural(managed: &ManagedProcess, exit_code: Option<i32>) -> ProcessOutcome {
    if managed.cancel_requested.load(Ordering::SeqCst) {
        return finalize_cancelled(managed, false, "cancelled");
    }
    let (status, terminal_state, reason) = match exit_code {
        Some(0) => (
            RunStatus::Completed,
            TerminalState::Completed,
            "completed".to_owned(),
        ),
        code => (
            RunStatus::Failed,
            TerminalState::Failed,
            format!("process exited with code {code:?}"),
        ),
    };
    finalize(
        managed,
        ProcessOutcome {
            status,
            terminal_state,
            exit_code,
            forced_stop_required: false,
            residual_detected: false,
            reason,
            stdout_lines: Vec::new(),
            stderr_lines: Vec::new(),
        },
    )
}

fn finalize_cancelled(managed: &ManagedProcess, forced: bool, reason: &str) -> ProcessOutcome {
    let exit_code = managed
        .child
        .lock()
        .expect("child mutex poisoned")
        .try_wait()
        .ok()
        .flatten()
        .and_then(|status| status.code());
    finalize(
        managed,
        ProcessOutcome {
            status: RunStatus::Cancelled,
            terminal_state: TerminalState::Cancelled,
            exit_code,
            forced_stop_required: forced,
            residual_detected: false,
            reason: reason.to_owned(),
            stdout_lines: Vec::new(),
            stderr_lines: Vec::new(),
        },
    )
}

fn finalize_uncertain(
    managed: &ManagedProcess,
    forced: bool,
    residual: bool,
    reason: String,
) -> ProcessOutcome {
    finalize_without_join(
        managed,
        ProcessOutcome {
            status: RunStatus::Uncertain,
            terminal_state: TerminalState::Unknown,
            exit_code: None,
            forced_stop_required: forced,
            residual_detected: residual,
            reason,
            stdout_lines: Vec::new(),
            stderr_lines: Vec::new(),
        },
    )
}

fn finalize_without_join(
    managed: &ManagedProcess,
    mut candidate: ProcessOutcome,
) -> ProcessOutcome {
    let mut outcome = managed.outcome.lock().expect("outcome mutex poisoned");
    if let Some(existing) = outcome.clone() {
        return existing;
    }
    candidate.stdout_lines = managed
        .stdout_lines
        .lock()
        .expect("stdout mutex poisoned")
        .clone();
    candidate.stderr_lines = managed
        .stderr_lines
        .lock()
        .expect("stderr mutex poisoned")
        .clone();
    *outcome = Some(candidate.clone());
    candidate
}

fn finalize(managed: &ManagedProcess, mut candidate: ProcessOutcome) -> ProcessOutcome {
    let mut outcome = managed.outcome.lock().expect("outcome mutex poisoned");
    if let Some(existing) = outcome.clone() {
        return existing;
    }
    for reader in managed
        .readers
        .lock()
        .expect("reader mutex poisoned")
        .drain(..)
    {
        let _ = reader.join();
    }
    candidate.stdout_lines = managed
        .stdout_lines
        .lock()
        .expect("stdout mutex poisoned")
        .clone();
    candidate.stderr_lines = managed
        .stderr_lines
        .lock()
        .expect("stderr mutex poisoned")
        .clone();
    *outcome = Some(candidate.clone());
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::AdapterKind;
    use std::path::PathBuf;

    fn shell_plan(script: &str) -> LaunchPlan {
        LaunchPlan {
            adapter: AdapterKind::Codex,
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: PathBuf::from("/tmp"),
            prompt_stdin: String::new(),
        }
    }

    #[test]
    fn captures_normal_exit_and_output() {
        let supervisor = ProcessSupervisor::new(Duration::from_millis(100), Duration::from_secs(1));
        supervisor
            .spawn("normal", shell_plan("printf 'hello\\n'"))
            .unwrap();
        let outcome = supervisor.wait("normal", Duration::from_secs(2)).unwrap();
        assert_eq!(outcome.status, RunStatus::Completed);
        assert_eq!(outcome.stdout_lines, vec!["hello"]);
    }

    #[test]
    fn cancellation_is_idempotent_and_wins_over_exit_code() {
        let supervisor = ProcessSupervisor::new(Duration::from_millis(100), Duration::from_secs(1));
        supervisor.spawn("cancel", shell_plan("sleep 30")).unwrap();
        let first = supervisor.cancel("cancel", "user").unwrap();
        let second = supervisor.cancel("cancel", "user").unwrap();
        assert_eq!(first.status, RunStatus::Cancelled);
        assert_eq!(first, second);
    }

    #[test]
    fn term_resistant_group_requires_force_and_leaves_no_confirmed_residual() {
        let supervisor = ProcessSupervisor::new(Duration::from_millis(100), Duration::from_secs(2));
        supervisor
            .spawn(
                "force",
                shell_plan("trap '' TERM; (trap '' TERM; sleep 30) & wait"),
            )
            .unwrap();
        thread::sleep(Duration::from_millis(100));
        let outcome = supervisor.cancel("force", "user").unwrap();
        assert!(outcome.forced_stop_required);
        assert_eq!(outcome.status, RunStatus::Cancelled);
        assert!(!outcome.residual_detected);
    }

    #[test]
    fn timeout_uses_cancellation_flow() {
        let supervisor = ProcessSupervisor::new(Duration::from_millis(100), Duration::from_secs(1));
        supervisor.spawn("timeout", shell_plan("sleep 30")).unwrap();
        let outcome = supervisor
            .wait("timeout", Duration::from_millis(100))
            .unwrap();
        assert_eq!(outcome.status, RunStatus::Cancelled);
        assert_eq!(outcome.reason, "timeout");
    }
}
