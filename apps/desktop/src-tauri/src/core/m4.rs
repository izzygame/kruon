use serde::{Deserialize, Serialize};

use super::domain::AdapterKind;

const CODEX_ALPHA_VERSIONS: &[&str] = &["0.144.1", "0.144.2"];
const CLAUDE_ALPHA_VERSIONS: &[&str] = &["2.1.205", "2.1.211"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityStatus {
    Supported,
    Unsupported,
    Unverified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionCompatibility {
    pub status: CompatibilityStatus,
    pub normalized_version: Option<String>,
    pub supported_versions: Vec<String>,
}

pub fn supported_versions(adapter: AdapterKind) -> &'static [&'static str] {
    match adapter {
        AdapterKind::Codex => CODEX_ALPHA_VERSIONS,
        AdapterKind::Claude => CLAUDE_ALPHA_VERSIONS,
    }
}

pub fn evaluate_version_output(adapter: AdapterKind, version_output: &str) -> VersionCompatibility {
    let supported = supported_versions(adapter);
    let candidates = version_candidates(version_output);
    let normalized_version = candidates
        .iter()
        .find(|candidate| supported.contains(&candidate.as_str()))
        .or_else(|| candidates.first())
        .cloned();
    let status = match normalized_version.as_deref() {
        Some(version) if supported.contains(&version) => CompatibilityStatus::Supported,
        Some(_) => CompatibilityStatus::Unsupported,
        None => CompatibilityStatus::Unverified,
    };
    VersionCompatibility {
        status,
        normalized_version,
        supported_versions: supported
            .iter()
            .map(|version| (*version).to_owned())
            .collect(),
    }
}

fn version_candidates(value: &str) -> Vec<String> {
    let bytes = value.as_bytes();
    let mut candidates = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        if !bytes[start].is_ascii_digit() {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
            end += 1;
        }
        let candidate = &value[start..end];
        let parts = candidate.split('.').collect::<Vec<_>>();
        let has_prerelease_suffix = bytes
            .get(end)
            .is_some_and(|byte| matches!(byte, b'-' | b'+'));
        if parts.len() == 3
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
            && !has_prerelease_suffix
        {
            candidates.push(candidate.to_owned());
        }
        start = end;
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::adapter_host::AdapterHost;
    use crate::core::domain::TerminalState;

    struct Fixture {
        adapter: AdapterKind,
        version: &'static str,
        body: &'static str,
    }

    const FIXTURES: &[Fixture] = &[
        Fixture {
            adapter: AdapterKind::Codex,
            version: "0.144.1",
            body: include_str!("../../fixtures/compatibility/codex-0.144.1.jsonl"),
        },
        Fixture {
            adapter: AdapterKind::Codex,
            version: "0.144.2",
            body: include_str!("../../fixtures/compatibility/codex-0.144.2.jsonl"),
        },
        Fixture {
            adapter: AdapterKind::Claude,
            version: "2.1.205",
            body: include_str!("../../fixtures/compatibility/claude-2.1.205.jsonl"),
        },
        Fixture {
            adapter: AdapterKind::Claude,
            version: "2.1.211",
            body: include_str!("../../fixtures/compatibility/claude-2.1.211.jsonl"),
        },
    ];

    #[test]
    fn parses_real_cli_version_shapes_and_rejects_unknown_or_prerelease_versions() {
        let codex = evaluate_version_output(AdapterKind::Codex, "codex-cli 0.144.1");
        assert_eq!(codex.status, CompatibilityStatus::Supported);
        assert_eq!(codex.normalized_version.as_deref(), Some("0.144.1"));

        let claude = evaluate_version_output(AdapterKind::Claude, "2.1.211 (Claude Code)");
        assert_eq!(claude.status, CompatibilityStatus::Supported);
        assert_eq!(claude.normalized_version.as_deref(), Some("2.1.211"));

        assert_eq!(
            evaluate_version_output(AdapterKind::Codex, "codex-cli 0.145.0").status,
            CompatibilityStatus::Unsupported
        );
        assert_eq!(
            evaluate_version_output(AdapterKind::Codex, "codex-cli 0.144.1-beta.1").status,
            CompatibilityStatus::Unverified
        );
        assert_eq!(
            evaluate_version_output(AdapterKind::Claude, "version unavailable").status,
            CompatibilityStatus::Unverified
        );
    }

    #[test]
    fn every_allowed_alpha_version_has_a_terminal_redacted_fixture() {
        for adapter in [AdapterKind::Codex, AdapterKind::Claude] {
            for version in supported_versions(adapter) {
                let fixture = FIXTURES
                    .iter()
                    .find(|fixture| fixture.adapter == adapter && fixture.version == *version)
                    .expect("every allowed version must have a fixture");
                let events = fixture
                    .body
                    .lines()
                    .enumerate()
                    .map(|(index, line)| {
                        AdapterHost.normalize_line(
                            adapter,
                            "compatibility-fixture",
                            index as u64 + 1,
                            line,
                        )
                    })
                    .collect::<Vec<_>>();
                assert!(!events.is_empty());
                assert!(events.iter().all(|event| event.payload["known"] == true));
                assert_eq!(
                    events.last().and_then(|event| event.terminal_state),
                    Some(TerminalState::Completed)
                );
                assert!(!events
                    .iter()
                    .any(|event| event.payload.to_string().contains("fixture-secret")));
            }
        }
    }
}
