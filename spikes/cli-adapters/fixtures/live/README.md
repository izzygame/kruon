# W1 real read-only captures

These files are redacted, normalized captures from one explicitly authorized real invocation per adapter on 2026-07-14.

- Codex: `codex-cli 0.144.2`, `codex exec --json --sandbox read-only --ephemeral`, 60-second Kruon timeout.
- Claude: Claude Code `2.1.205`, `stream-json`, `permission-mode=plan`, no session persistence, USD 0.10 maximum budget, 60-second Kruon timeout.
- Each invocation used a separate disposable system-temp directory containing only `README.txt` and a non-secret marker.
- Kruon fingerprinted every fixture file before and after execution; both captures report `fixture_unchanged=true`.
- Absolute user/temp paths and process identifiers were replaced. Secret-shaped fields were redacted recursively before the capture was written.
- The files contain normalized events, not the raw CLI byte stream. Codex non-JSON stderr is represented only by SHA-256 diagnostics.
- Each capture records the CLI version, protocol surface, and normalized event schema version.

These captures prove the W1 read-only launch, normalization, SQLite persistence, terminal projection, and replay path on the recorded CLI versions. They do not prove network egress isolation or stable compatibility with future CLI versions.
