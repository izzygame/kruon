# ADR-008: Alpha CLI version compatibility gate

- Status: Accepted
- Date: 2026-07-16
- Scope: M4 DEV-401

## Context

Finding a `codex` or `claude` executable and obtaining version text does not prove that Kruon's frozen arguments, structured event parser, terminal mapping, redaction, and cancellation assumptions still hold. Upstream CLIs can change independently of Kruon. Treating every successfully probed version as ready would turn an upstream upgrade into an unreviewed execution-contract change.

## Decision

1. Alpha compatibility is an exact patch-version allowlist, not an open semantic-version range.
2. The initial matrix allows Codex `0.144.1` and `0.144.2`, and Claude Code `2.1.205` and `2.1.211`.
3. Every allowed version must have a checked-in structured-output fixture that reaches one known terminal event and passes the common redaction path.
4. A successful version probe is parsed independently of surrounding product text. Empty, unparseable, prerelease, and non-allowlisted versions are not ready.
5. The connection panel displays the compatibility result and allowed versions. Run buttons are disabled unless the selected adapter is ready.
6. The Rust runtime repeats the compatibility gate immediately before creating a Run or spawning a process. Frontend state is never the sole enforcement point.
7. Compatibility failures expose a normalized public error code without echoing executable paths or raw probe output.

## Consequences

- A silent upstream CLI update can temporarily block new Runs until its fixture and controlled probe are reviewed.
- Existing local Run history remains readable because the gate affects only new execution.
- Exact versions create maintenance work, but the Alpha contract remains explicit and fail-closed.
- Fixture success proves parser-contract coverage, not the upstream provider's network availability, authentication, or complete behavioral equivalence.

## Evidence

- `src/core/m4.rs` owns the frozen matrix, version parser, and matrix/fixture invariant tests.
- `fixtures/compatibility/` contains one redacted terminal fixture for every allowed patch.
- `m1.rs` projects compatibility into connection status; `runtime.rs` enforces it before Run creation.
- The M4 verification record captures automated and live local evidence.

## Revisit triggers

- adding or removing an allowed CLI version;
- changing fixed launch arguments or known event types;
- enabling prerelease versions or semantic-version ranges;
- adding an adapter beyond Codex and Claude Code;
- introducing a signed compatibility manifest or controlled remote update channel.
