use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::domain::{AdapterKind, EventEnvelope, EventPhase, TerminalState};
use super::error::{KruonError, KruonResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchPlan {
    pub adapter: AdapterKind,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub prompt_stdin: String,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAdapterProgram {
    pub executable: PathBuf,
    pub source: &'static str,
}

impl LaunchPlan {
    /// Fingerprint the frozen executable contract without persisting the prompt.
    pub fn fingerprint(&self) -> String {
        let prompt_hash = format!("{:x}", Sha256::digest(self.prompt_stdin.as_bytes()));
        let canonical = serde_json::json!({
            "adapter": self.adapter,
            "program": &self.program,
            "args": &self.args,
            "cwd": &self.cwd,
            "prompt_hash": prompt_hash,
            "environment_keys": self.env.iter().map(|(key, _)| key).collect::<Vec<_>>(),
        });
        format!("{:x}", Sha256::digest(canonical.to_string().as_bytes()))
    }
}

#[derive(Debug, Clone, Default)]
pub struct AdapterHost;

impl AdapterHost {
    pub fn launch_plan(
        &self,
        adapter: AdapterKind,
        workspace: &Path,
        prompt: &str,
    ) -> KruonResult<LaunchPlan> {
        if prompt.trim().is_empty() {
            return Err(KruonError::InvalidArgument(
                "prompt must not be empty".into(),
            ));
        }
        let program = resolve_adapter_program(adapter).ok_or_else(|| {
            KruonError::Adapter(format!(
                "{} CLI was not found in PATH or common per-user installation locations",
                adapter.as_str()
            ))
        })?;
        self.launch_plan_for_program(adapter, workspace, prompt, program.executable)
    }

    fn launch_plan_for_program(
        &self,
        adapter: AdapterKind,
        workspace: &Path,
        prompt: &str,
        program: PathBuf,
    ) -> KruonResult<LaunchPlan> {
        if prompt.trim().is_empty() {
            return Err(KruonError::InvalidArgument(
                "prompt must not be empty".into(),
            ));
        }
        let workspace = workspace
            .to_str()
            .ok_or_else(|| KruonError::Adapter("workspace path is not UTF-8".into()))?;
        let args = match adapter {
            AdapterKind::Codex => vec![
                "exec".into(),
                "--json".into(),
                "--sandbox".into(),
                "read-only".into(),
                "--ephemeral".into(),
                "-C".into(),
                workspace.into(),
                "--skip-git-repo-check".into(),
                "-".into(),
            ],
            AdapterKind::Claude => vec![
                "-p".into(),
                "--output-format".into(),
                "stream-json".into(),
                "--verbose".into(),
                "--permission-mode".into(),
                "plan".into(),
                "--no-session-persistence".into(),
                "--no-chrome".into(),
                "--max-budget-usd".into(),
                "0.10".into(),
            ],
        };
        Ok(LaunchPlan {
            adapter,
            program: program.to_string_lossy().into_owned(),
            args,
            cwd: PathBuf::from(workspace),
            prompt_stdin: prompt.to_owned(),
            env: adapter_environment(&program),
        })
    }

    pub fn normalize_line(
        &self,
        adapter: AdapterKind,
        run_id: &str,
        sequence: u64,
        line: &str,
    ) -> EventEnvelope {
        let parsed: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(_) => {
                return EventEnvelope::new(
                    run_id,
                    sequence,
                    "adapter.malformed_json",
                    EventPhase::Degraded,
                    None,
                    serde_json::json!({
                        "adapter": adapter,
                        "line_sha256": format!("{:x}", Sha256::digest(line.as_bytes())),
                    }),
                )
            }
        };

        let redacted = redact_value(parsed);
        let source_type = source_type(&redacted).unwrap_or("unknown");
        let lower = source_type.to_ascii_lowercase();
        let known = is_known(adapter, &lower);
        let terminal_state = terminal_from_type(&lower, &redacted);
        let phase = if !known {
            EventPhase::Degraded
        } else {
            phase_from_type(&lower, terminal_state)
        };
        EventEnvelope::new(
            run_id,
            sequence,
            format!("{}.{}", adapter.as_str(), source_type),
            phase,
            terminal_state,
            serde_json::json!({
                "adapter": adapter,
                "source_event_type": source_type,
                "source": redacted,
                "known": known,
            }),
        )
    }
}

pub fn resolve_adapter_program(adapter: AdapterKind) -> Option<ResolvedAdapterProgram> {
    let directories = adapter_search_directories();
    for directory in directories {
        if let Some(executable) =
            resolve_in_directories(adapter, std::slice::from_ref(&directory.path))
        {
            return Some(ResolvedAdapterProgram {
                executable,
                source: directory.source,
            });
        }
    }
    None
}

pub fn adapter_environment(program: &Path) -> Vec<(String, String)> {
    let mut environment = base_environment();
    // The npm command shim invokes `node` by name. Put the resolved shim and
    // runtime first so a GUI-inherited app runtime cannot shadow either one.
    let mut paths = Vec::new();
    if let Some(parent) = program.parent() {
        paths.push(parent.to_path_buf());
    }
    if let Some(node) = resolve_program_by_name("node") {
        if let Some(parent) = node.parent() {
            paths.push(parent.to_path_buf());
        }
    }
    paths.extend(current_path_entries());
    let paths = unique_paths(paths);
    if let Ok(path) = std::env::join_paths(paths) {
        environment.retain(|(key, _)| key != "PATH");
        environment.push(("PATH".into(), path.to_string_lossy().into_owned()));
    }
    environment
}

const ALLOWED_ENVIRONMENT_KEYS: &[&str] = &[
    "PATH",
    "HOME",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TERM",
    "COLORTERM",
    "NO_COLOR",
    "USER",
    "LOGNAME",
    "SHELL",
    "CODEX_HOME",
    "CLAUDE_CONFIG_DIR",
    "APPDATA",
    "LOCALAPPDATA",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "TEMP",
    "TMP",
    "SystemRoot",
    "COMSPEC",
    "PATHEXT",
];

fn base_environment() -> Vec<(String, String)> {
    ALLOWED_ENVIRONMENT_KEYS
        .iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| ((*key).into(), value)))
        .collect()
}

#[derive(Debug, Clone)]
struct SearchDirectory {
    path: PathBuf,
    source: &'static str,
}

fn adapter_search_directories() -> Vec<SearchDirectory> {
    // A desktop app can inherit a PATH entry for its own packaged resources
    // before the user-installed CLI shim. Prefer the conventional per-user
    // bins so `codex.cmd` / `claude.cmd` win over an embedded app resource.
    let mut directories = common_user_bin_directories()
        .into_iter()
        .map(|path| SearchDirectory {
            path,
            source: "common per-user installation location",
        })
        .collect::<Vec<_>>();
    for path in current_path_entries() {
        directories.push(SearchDirectory {
            path,
            source: "PATH",
        });
    }
    unique_search_directories(directories)
}

fn resolve_program_by_name(name: &str) -> Option<PathBuf> {
    let directories = adapter_search_directories()
        .into_iter()
        .map(|directory| directory.path)
        .collect::<Vec<_>>();
    resolve_named_program(name, &directories)
}

fn resolve_in_directories(adapter: AdapterKind, directories: &[PathBuf]) -> Option<PathBuf> {
    resolve_named_program(adapter.as_str(), directories)
}

fn resolve_named_program(name: &str, directories: &[PathBuf]) -> Option<PathBuf> {
    for directory in directories {
        for file_name in executable_names(name) {
            let candidate = directory.join(file_name);
            if candidate.is_file() {
                return candidate.canonicalize().ok().or(Some(candidate));
            }
        }
    }
    None
}

#[cfg(windows)]
fn executable_names(name: &str) -> Vec<OsString> {
    [".exe", ".cmd", ".bat", ""]
        .into_iter()
        .map(|extension| OsString::from(format!("{name}{extension}")))
        .collect()
}

#[cfg(not(windows))]
fn executable_names(name: &str) -> Vec<OsString> {
    vec![OsString::from(name)]
}

fn current_path_entries() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default()
}

fn common_user_bin_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(app_data) = std::env::var_os("APPDATA") {
        directories.push(PathBuf::from(app_data).join("npm"));
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        directories.push(PathBuf::from(&local_app_data).join("pnpm"));
        directories.push(
            PathBuf::from(local_app_data)
                .join("Programs")
                .join("nodejs"),
        );
    }
    if let Some(home) = user_home_directory() {
        directories.push(home.join("AppData").join("Roaming").join("npm"));
        directories.push(home.join("AppData").join("Local").join("pnpm"));
        directories.push(home.join(".local").join("bin"));
        directories.push(home.join(".cargo").join("bin"));
    }
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        directories.push(PathBuf::from(program_files).join("nodejs"));
    }
    if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
        directories.push(PathBuf::from(program_files_x86).join("nodejs"));
    }
    unique_paths(directories)
}

fn user_home_directory() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn unique_search_directories(directories: Vec<SearchDirectory>) -> Vec<SearchDirectory> {
    let mut seen = HashSet::new();
    directories
        .into_iter()
        .filter(|directory| seen.insert(path_key(&directory.path)))
        .collect()
}

fn unique_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path_key(path)))
        .collect()
}

fn path_key(path: &Path) -> OsString {
    #[cfg(windows)]
    {
        return OsString::from(path.to_string_lossy().to_ascii_lowercase());
    }
    #[cfg(not(windows))]
    {
        path.as_os_str().to_owned()
    }
}

fn source_type(value: &Value) -> Option<&str> {
    value
        .get("type")
        .or_else(|| value.get("event"))
        .or_else(|| value.get("event_type"))
        .and_then(Value::as_str)
}

fn is_known(adapter: AdapterKind, event_type: &str) -> bool {
    match adapter {
        AdapterKind::Codex => {
            event_type.starts_with("thread.")
                || event_type.starts_with("turn.")
                || event_type.starts_with("item.")
                || event_type == "error"
        }
        AdapterKind::Claude => matches!(
            event_type,
            "system" | "assistant" | "user" | "result" | "stream_event" | "error"
        ),
    }
}

fn terminal_from_type(event_type: &str, value: &Value) -> Option<TerminalState> {
    if event_type.contains("cancel") {
        return Some(TerminalState::Cancelled);
    }
    if event_type == "error" || event_type.contains("failed") {
        return Some(TerminalState::Failed);
    }
    if event_type == "result" {
        return match value.get("is_error").and_then(Value::as_bool) {
            Some(true) => Some(TerminalState::Failed),
            _ => Some(TerminalState::Completed),
        };
    }
    if event_type == "turn.completed" || event_type.ends_with(".completed") {
        return Some(TerminalState::Completed);
    }
    None
}

fn phase_from_type(event_type: &str, terminal: Option<TerminalState>) -> EventPhase {
    if terminal.is_some() {
        return EventPhase::Terminal;
    }
    if event_type.contains("approval") || event_type.contains("permission") {
        EventPhase::WaitingApproval
    } else if event_type.contains("tool") || event_type.contains("item") {
        EventPhase::ToolCall
    } else if event_type.contains("thread") || event_type == "system" {
        EventPhase::Setup
    } else {
        EventPhase::Running
    }
}

fn redact_value(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut redacted = Map::with_capacity(object.len());
            for (key, value) in object {
                if is_secret_key(&key) {
                    redacted.insert(key, Value::String("[REDACTED]".into()));
                } else {
                    redacted.insert(key, redact_value(value));
                }
            }
            Value::Object(redacted)
        }
        Value::Array(values) => Value::Array(values.into_iter().map(redact_value).collect()),
        other => other,
    }
}

fn is_secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace('-', "_");
    matches!(
        key.as_str(),
        "token"
            | "access_token"
            | "refresh_token"
            | "api_key"
            | "authorization"
            | "password"
            | "secret"
            | "credential"
            | "credentials"
            | "private_key"
            | "ssh_key"
            | "session_key"
            | "client_secret"
    ) || key.ends_with("_token")
        || key.ends_with("_secret")
        || key.ends_with("_password")
        || key.ends_with("_api_key")
        || key.contains("credential")
        || key.contains("authorization")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_are_fixed_and_do_not_put_prompt_in_arguments() {
        let host = AdapterHost;
        let codex = host
            .launch_plan_for_program(
                AdapterKind::Codex,
                Path::new("/tmp"),
                "sensitive prompt",
                PathBuf::from("codex"),
            )
            .unwrap();
        assert_eq!(codex.program, "codex");
        assert!(codex
            .args
            .windows(2)
            .any(|pair| pair == ["--sandbox", "read-only"]));
        assert!(!codex.args.iter().any(|arg| arg.contains("sensitive")));
        assert!(!codex.args.iter().any(|arg| arg.contains("bypass")));
        let env_keys = codex
            .env
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>();
        assert!(!env_keys.contains(&"OPENAI_API_KEY"));
        assert!(!env_keys.contains(&"ANTHROPIC_API_KEY"));
        assert!(!env_keys.contains(&"SSH_AUTH_SOCK"));

        let claude = host
            .launch_plan_for_program(
                AdapterKind::Claude,
                Path::new("/tmp"),
                "sensitive prompt",
                PathBuf::from("claude"),
            )
            .unwrap();
        assert_eq!(claude.program, "claude");
        assert!(claude
            .args
            .windows(2)
            .any(|pair| pair == ["--permission-mode", "plan"]));
        assert!(!claude.args.iter().any(|arg| arg.contains("sensitive")));
        assert!(!claude.args.iter().any(|arg| arg.contains("bypass")));
    }

    #[cfg(windows)]
    #[test]
    fn resolves_windows_npm_command_shims_from_a_known_bin_directory() {
        let directory = tempfile::tempdir().unwrap();
        let shim = directory.path().join("codex.cmd");
        std::fs::write(&shim, "@echo off\r\n").unwrap();
        let resolved =
            resolve_in_directories(AdapterKind::Codex, &[directory.path().to_path_buf()]).unwrap();
        assert_eq!(resolved, shim.canonicalize().unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn prioritizes_a_user_cli_shim_over_a_path_resource() {
        let root = tempfile::tempdir().unwrap();
        let user_bin = root.path().join("npm");
        let path_resource = root.path().join("app-resources");
        std::fs::create_dir_all(&user_bin).unwrap();
        std::fs::create_dir_all(&path_resource).unwrap();
        let shim = user_bin.join("codex.cmd");
        std::fs::write(&shim, "@echo off\r\n").unwrap();
        std::fs::write(path_resource.join("codex.exe"), "not a CLI").unwrap();

        let resolved =
            resolve_in_directories(AdapterKind::Codex, &[user_bin, path_resource]).unwrap();
        assert_eq!(resolved, shim.canonicalize().unwrap());
    }

    #[test]
    fn execution_environment_includes_the_resolved_program_parent() {
        let directory = tempfile::tempdir().unwrap();
        let program = directory.path().join("codex");
        let environment = adapter_environment(&program);
        let path = environment
            .iter()
            .find(|(key, _)| key == "PATH")
            .map(|(_, value)| value)
            .unwrap();
        let entries = std::env::split_paths(path).collect::<Vec<_>>();
        assert_eq!(entries.first(), Some(&directory.path().to_path_buf()));
        assert!(entries.iter().any(|entry| entry == directory.path()));
    }

    #[test]
    fn redacts_nested_secrets_and_preserves_unknown_events() {
        let event = AdapterHost.normalize_line(
            AdapterKind::Claude,
            "run-1",
            1,
            r#"{"type":"future_event","api_key":"abc","nested":{"token":"def"}}"#,
        );
        assert_eq!(event.phase, EventPhase::Degraded);
        let source = &event.payload["source"];
        assert_eq!(source["api_key"], "[REDACTED]");
        assert_eq!(source["nested"]["token"], "[REDACTED]");
    }

    #[test]
    fn redacts_secret_suffixes_without_redacting_unrelated_keys() {
        let event = AdapterHost.normalize_line(
            AdapterKind::Codex,
            "run-1",
            1,
            r#"{"type":"item.completed","oauth_token":"a","client-secret":"b","database_password":"c","private_key":"d","keyboard":"keep"}"#,
        );
        let source = &event.payload["source"];
        assert_eq!(source["oauth_token"], "[REDACTED]");
        assert_eq!(source["client-secret"], "[REDACTED]");
        assert_eq!(source["database_password"], "[REDACTED]");
        assert_eq!(source["private_key"], "[REDACTED]");
        assert_eq!(source["keyboard"], "keep");
    }

    #[test]
    fn malformed_lines_store_only_a_hash() {
        let event = AdapterHost.normalize_line(AdapterKind::Codex, "run-1", 1, "secret raw");
        assert_eq!(event.phase, EventPhase::Degraded);
        assert!(!event.payload.to_string().contains("secret raw"));
    }
}
