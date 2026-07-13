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
        let workspace = workspace
            .to_str()
            .ok_or_else(|| KruonError::Adapter("workspace path is not UTF-8".into()))?;
        let (program, args) = match adapter {
            AdapterKind::Codex => (
                "codex".to_owned(),
                vec![
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
            ),
            AdapterKind::Claude => (
                "claude".to_owned(),
                vec![
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
            ),
        };
        Ok(LaunchPlan {
            adapter,
            program,
            args,
            cwd: PathBuf::from(workspace),
            prompt_stdin: prompt.to_owned(),
            env: allowed_environment(),
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
];

fn allowed_environment() -> Vec<(String, String)> {
    ALLOWED_ENVIRONMENT_KEYS
        .iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| ((*key).into(), value)))
        .collect()
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
            .launch_plan(AdapterKind::Codex, Path::new("/tmp"), "sensitive prompt")
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
            .launch_plan(AdapterKind::Claude, Path::new("/tmp"), "sensitive prompt")
            .unwrap();
        assert_eq!(claude.program, "claude");
        assert!(claude
            .args
            .windows(2)
            .any(|pair| pair == ["--permission-mode", "plan"]));
        assert!(!claude.args.iter().any(|arg| arg.contains("sensitive")));
        assert!(!claude.args.iter().any(|arg| arg.contains("bypass")));
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
