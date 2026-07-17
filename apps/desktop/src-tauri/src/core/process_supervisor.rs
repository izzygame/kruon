use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::adapter_host::LaunchPlan;
use super::domain::{RunStatus, TerminalState};
use super::error::{KruonError, KruonResult};

pub const MAX_CAPTURED_LINE_BYTES: usize = 256 * 1024;
pub const MAX_CAPTURED_STREAM_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_CAPTURED_STREAM_LINES: usize = 10_000;

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
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub stdout_lossy_lines: usize,
    pub stderr_lossy_lines: usize,
}

#[derive(Debug, Clone, Default)]
struct CapturedStream {
    lines: Vec<String>,
    captured_bytes: usize,
    truncated: bool,
    lossy_lines: usize,
}

struct ManagedProcess {
    pid: u32,
    pgid: i32,
    child: Mutex<Child>,
    cancel_requested: AtomicBool,
    outcome: Mutex<Option<ProcessOutcome>>,
    stdout: Arc<Mutex<CapturedStream>>,
    stderr: Arc<Mutex<CapturedStream>>,
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
            .env_clear()
            .envs(plan.env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(|error| {
            KruonError::Process(format!("failed to spawn {}: {error}", plan.program))
        })?;
        let pid = child.id();
        let pgid = i32::try_from(pid)
            .map_err(|_| KruonError::Process(format!("PID {pid} does not fit in i32")))?;

        let stdin = child.stdin.take();

        let stdout = Arc::new(Mutex::new(CapturedStream::default()));
        let stderr = Arc::new(Mutex::new(CapturedStream::default()));
        let mut readers = Vec::new();
        if let Some(reader) = child.stdout.take() {
            readers.push(drain_output(reader, Arc::clone(&stdout)));
        }
        if let Some(reader) = child.stderr.take() {
            readers.push(drain_output(reader, Arc::clone(&stderr)));
        }

        let managed = Arc::new(ManagedProcess {
            pid,
            pgid,
            child: Mutex::new(child),
            cancel_requested: AtomicBool::new(false),
            outcome: Mutex::new(None),
            stdout,
            stderr,
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
            drop(stdin);
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

        let term_result = terminate_owned_group(&managed, false);
        if let Err(error) = term_result {
            return Ok(finalize_uncertain(
                &managed,
                false,
                true,
                format!("{reason}: graceful termination failed: {error}"),
            ));
        }
        if wait_for_exit(&managed, self.term_grace)? {
            return Ok(finalize_cancelled(&managed, false, reason));
        }

        if let Err(error) = terminate_owned_group(&managed, true) {
            return Ok(finalize_uncertain(
                &managed,
                true,
                true,
                format!("{reason}: forced termination failed: {error}"),
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

fn drain_output<R: Read + Send + 'static>(
    reader: R,
    target: Arc<Mutex<CapturedStream>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = reader;
        let mut chunk = [0_u8; 8 * 1024];
        let mut line = Vec::with_capacity(8 * 1024);
        let mut line_truncated = false;
        loop {
            let count = match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(count) => count,
                Err(_) => {
                    target.lock().expect("output mutex poisoned").truncated = true;
                    return;
                }
            };
            for byte in &chunk[..count] {
                if *byte == b'\n' {
                    capture_line(&target, &mut line, line_truncated);
                    line_truncated = false;
                } else if line.len() < MAX_CAPTURED_LINE_BYTES {
                    line.push(*byte);
                } else {
                    line_truncated = true;
                }
            }
        }
        if !line.is_empty() || line_truncated {
            capture_line(&target, &mut line, line_truncated);
        }
    })
}

fn capture_line(target: &Arc<Mutex<CapturedStream>>, line: &mut Vec<u8>, line_truncated: bool) {
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    let mut capture = target.lock().expect("output mutex poisoned");
    if capture.lines.len() >= MAX_CAPTURED_STREAM_LINES
        || capture.captured_bytes >= MAX_CAPTURED_STREAM_BYTES
    {
        capture.truncated = true;
        line.clear();
        return;
    }
    let remaining = MAX_CAPTURED_STREAM_BYTES - capture.captured_bytes;
    let captured_length = line.len().min(remaining);
    let captured = &line[..captured_length];
    if std::str::from_utf8(captured).is_err() {
        capture.lossy_lines += 1;
    }
    capture
        .lines
        .push(String::from_utf8_lossy(captured).into_owned());
    capture.captured_bytes += captured_length;
    capture.truncated |= line_truncated || captured_length < line.len();
    line.clear();
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

#[cfg(unix)]
fn terminate_owned_group(managed: &ManagedProcess, force: bool) -> KruonResult<()> {
    if managed.pgid <= 1 || managed.pgid != managed.pid as i32 {
        return Err(KruonError::Process(format!(
            "refusing to signal unowned process group {} for PID {}",
            managed.pgid, managed.pid
        )));
    }
    let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
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

#[cfg(windows)]
fn terminate_owned_group(managed: &ManagedProcess, force: bool) -> KruonResult<()> {
    let mut command = Command::new("taskkill");
    command.arg("/PID").arg(managed.pid.to_string()).arg("/T");
    if force {
        command.arg("/F");
    }
    let status = command.status()?;
    if status.success()
        || managed
            .child
            .lock()
            .expect("child mutex poisoned")
            .try_wait()?
            .is_some()
    {
        return Ok(());
    }
    Err(KruonError::Process(format!(
        "taskkill did not terminate process tree for PID {}",
        managed.pid
    )))
}

#[cfg(unix)]
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

#[cfg(windows)]
fn group_exists(_pgid: i32) -> KruonResult<bool> {
    // `taskkill /T` is the Windows process-tree primitive used above. Windows
    // does not expose POSIX process groups, so only report a residual when the
    // owned root process is still alive; otherwise the tree kill was accepted.
    Ok(false)
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
            stdout_truncated: false,
            stderr_truncated: false,
            stdout_lossy_lines: 0,
            stderr_lossy_lines: 0,
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
            stdout_truncated: false,
            stderr_truncated: false,
            stdout_lossy_lines: 0,
            stderr_lossy_lines: 0,
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
            stdout_truncated: false,
            stderr_truncated: false,
            stdout_lossy_lines: 0,
            stderr_lossy_lines: 0,
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
    apply_captured_output(managed, &mut candidate);
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
    apply_captured_output(managed, &mut candidate);
    *outcome = Some(candidate.clone());
    candidate
}

fn apply_captured_output(managed: &ManagedProcess, candidate: &mut ProcessOutcome) {
    let stdout = managed
        .stdout
        .lock()
        .expect("stdout mutex poisoned")
        .clone();
    let stderr = managed
        .stderr
        .lock()
        .expect("stderr mutex poisoned")
        .clone();
    candidate.stdout_lines = stdout.lines;
    candidate.stderr_lines = stderr.lines;
    candidate.stdout_truncated = stdout.truncated;
    candidate.stderr_truncated = stderr.truncated;
    candidate.stdout_lossy_lines = stdout.lossy_lines;
    candidate.stderr_lossy_lines = stderr.lossy_lines;
}

#[cfg(test)]
mod capture_tests {
    use std::io::Cursor;

    use super::*;

    fn capture(bytes: Vec<u8>) -> CapturedStream {
        let target = Arc::new(Mutex::new(CapturedStream::default()));
        drain_output(Cursor::new(bytes), Arc::clone(&target))
            .join()
            .unwrap();
        let result = target.lock().unwrap().clone();
        result
    }

    #[test]
    fn invalid_utf8_is_lossy_and_does_not_drop_following_events() {
        let result = capture(
            b"{\"type\":\"thread.started\"}\n\xff\xfe\n{\"type\":\"turn.completed\"}\n".to_vec(),
        );
        assert_eq!(result.lines.len(), 3);
        assert_eq!(result.lossy_lines, 1);
        assert!(result.lines[1].contains('\u{fffd}'));
        assert!(result.lines[2].contains("turn.completed"));
    }

    #[test]
    fn oversized_lines_and_line_counts_are_bounded_but_fully_drained() {
        let mut long = vec![b'a'; MAX_CAPTURED_LINE_BYTES + 4_096];
        long.extend_from_slice(b"\ntail\n");
        let result = capture(long);
        assert_eq!(result.lines[0].len(), MAX_CAPTURED_LINE_BYTES);
        assert_eq!(result.lines[1], "tail");
        assert!(result.truncated);

        let many = "x\n".repeat(MAX_CAPTURED_STREAM_LINES + 1).into_bytes();
        let many_result = capture(many);
        assert_eq!(many_result.lines.len(), MAX_CAPTURED_STREAM_LINES);
        assert!(many_result.truncated);
    }

    #[test]
    fn total_stream_bytes_are_bounded_independently_of_line_count() {
        let line = vec![b'b'; 1024];
        let mut bytes = Vec::with_capacity(MAX_CAPTURED_STREAM_BYTES + 8 * 1024);
        for _ in 0..=(MAX_CAPTURED_STREAM_BYTES / line.len()) {
            bytes.extend_from_slice(&line);
            bytes.push(b'\n');
        }
        let result = capture(bytes);
        assert_eq!(result.captured_bytes, MAX_CAPTURED_STREAM_BYTES);
        assert!(result.lines.len() < MAX_CAPTURED_STREAM_LINES);
        assert!(result.truncated);
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use crate::core::domain::AdapterKind;

    fn cmd_plan(script: &str) -> LaunchPlan {
        let program = std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into());
        let mut env = Vec::new();
        for key in ["SystemRoot", "PATH"] {
            if let Ok(value) = std::env::var(key) {
                env.push((key.to_owned(), value));
            }
        }
        LaunchPlan {
            adapter: AdapterKind::Codex,
            program,
            args: vec!["/D".into(), "/S".into(), "/C".into(), script.into()],
            cwd: std::env::temp_dir(),
            prompt_stdin: String::new(),
            env,
        }
    }

    #[test]
    fn windows_nonzero_exit_is_failed_and_preserves_output() {
        let supervisor = ProcessSupervisor::new(Duration::from_millis(100), Duration::from_secs(1));
        supervisor
            .spawn("windows-crash", cmd_plan("echo crash-fixture & exit /b 7"))
            .unwrap();
        let outcome = supervisor
            .wait("windows-crash", Duration::from_secs(2))
            .unwrap();
        assert_eq!(outcome.status, RunStatus::Failed);
        assert_eq!(outcome.terminal_state, TerminalState::Failed);
        assert_eq!(outcome.exit_code, Some(7));
        assert_eq!(outcome.stdout_lines, vec!["crash-fixture "]);
    }

    #[test]
    fn windows_timeout_never_becomes_completed() {
        let supervisor = ProcessSupervisor::new(Duration::from_millis(100), Duration::from_secs(1));
        supervisor
            .spawn("windows-timeout", cmd_plan("ping 127.0.0.1 -n 6 >nul"))
            .unwrap();
        let outcome = supervisor
            .wait("windows-timeout", Duration::from_millis(100))
            .unwrap();
        assert_ne!(outcome.status, RunStatus::Completed);
        assert_ne!(outcome.terminal_state, TerminalState::Completed);
        assert!(outcome.reason.contains("timeout"));
    }
}

#[cfg(all(test, unix))]
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
            env: vec![("PATH".into(), "/usr/bin:/bin".into())],
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
    fn closes_stdin_and_does_not_inherit_unlisted_secrets() {
        let supervisor = ProcessSupervisor::new(Duration::from_millis(100), Duration::from_secs(1));
        let mut plan = shell_plan(
            "IFS= read -r prompt; IFS= read -r trailing || printf 'eof:%s:%s\\n' \"$prompt\" \"${KRUON_TEST_SECRET-unset}\"",
        );
        plan.prompt_stdin = "hello".into();
        unsafe { std::env::set_var("KRUON_TEST_SECRET", "must-not-leak") };
        supervisor.spawn("stdin-eof", plan).unwrap();
        let outcome = supervisor
            .wait("stdin-eof", Duration::from_secs(2))
            .unwrap();
        unsafe { std::env::remove_var("KRUON_TEST_SECRET") };
        assert_eq!(outcome.status, RunStatus::Completed);
        assert_eq!(outcome.stdout_lines, vec!["eof:hello:unset"]);
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
    fn concurrent_cancel_and_wait_produce_one_stable_outcome() {
        let supervisor = Arc::new(ProcessSupervisor::new(
            Duration::from_millis(100),
            Duration::from_secs(1),
        ));
        supervisor.spawn("race", shell_plan("sleep 30")).unwrap();
        let waiter = {
            let supervisor = Arc::clone(&supervisor);
            thread::spawn(move || supervisor.wait("race", Duration::from_secs(2)).unwrap())
        };
        let cancelled = supervisor.cancel("race", "user").unwrap();
        let waited = waiter.join().unwrap();
        assert_eq!(cancelled, waited);
        assert_eq!(cancelled.status, RunStatus::Cancelled);
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
