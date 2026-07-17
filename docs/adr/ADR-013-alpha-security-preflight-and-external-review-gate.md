# ADR-013: Alpha security preflight and independent review gate

- Status: Accepted with external review gate
- Date: 2026-07-17
- Scope: M4 DEV-406

## Context

Kruon controls third-party coding CLIs inside user-selected workspaces. Internal tests can prove repository invariants, but the project cannot independently validate its own threat assumptions or call a self-review “external security review.” The Alpha also retains local history that may contain task and run metadata, so default filesystem behavior is not a sufficient storage policy.

## Decision

1. DEV-406 has two distinct gates: an internal repository preflight and an independent external review of a frozen commit. Passing the preflight does not close the external gate.
2. Every on-disk SQLite connection uses `SQLITE_OPEN_NOFOLLOW`. The immediate database directory and database file are rejected when they are symbolic links. On Unix/macOS the Kruon data directory is forced to `0700` and the database file to `0600`.
3. Workspace trust is revocable from the main control window. Revocation blocks new enqueue requests and is checked again before a durable queued entry launches. It does not silently claim to terminate a Run that is already active; the user must cancel that Run explicitly.
4. CI runs a fail-closed repository security-contract check. It freezes the main/world capability split, keeps direct `start_run` unregistered, checks the WebView script/network CSP boundary, requires the fixed read-only adapter flags and cleared process environment, and requires the SQLite no-follow/private-mode contract.
5. Exact CLI version checks are compatibility evidence, not binary-integrity evidence. A compromised executable able to spoof a supported version remains an explicit risk for external review.
6. Network egress, runtime filesystem TOCTOU, process-group escape, and the absence of an OS-level sandbox remain high residual risks. They block any claim of complete isolation and must be revisited before write-capable adapters, automatic approval, or broad public release.
7. Dependency advisory checks are evidence with a timestamp, not a permanent guarantee. The Node production graph is checked against the official npm registry for this preflight; RustSec automation remains an external-review/CI follow-up until a pinned audit tool is added.

## Consequences

- A database symlink substitution now fails before schema or product operations, and local metadata is private by default on the Alpha macOS target.
- Trust can be withdrawn without deleting history, while active-process state remains explicit.
- Capability, CSP, read-only-plan, environment, and storage regressions fail in pull requests and the signed macOS workflow.
- The review packet records open high risks instead of reducing their severity because the current adapters are nominally read-only.
- DEV-406 remains open until an independent reviewer examines a frozen commit and all accepted high/critical findings have evidence-backed dispositions.

## Evidence

- `database.rs` owns the shared on-disk SQLite opener and its platform permission tests.
- `RuntimeCore::untrust_workspace`, the main Tauri command/capability, and the workspace UI expose trust revocation.
- `scripts/check-alpha-security.mjs` and its mutation tests define the repository security drift gate.
- `docs/security/M4-external-security-review-packet-2026-07-17.md` contains scope, threats, commands, requested deliverables, and risk ownership.

## Revisit triggers

- enabling workspace-write, shell, browser, MCP, or network-capable product adapters;
- adding automatic approval, unattended background execution, sync, telemetry, or remote control;
- changing Tauri windows/capabilities, CSP, updater behavior, database location, or credential handling;
- expanding the CLI version matrix or adding a local-model/provider adapter;
- receiving an external critical/high finding or a dependency advisory affecting the release graph.
