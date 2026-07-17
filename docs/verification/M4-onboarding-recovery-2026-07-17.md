# M4 first-connection onboarding and recovery verification

- Date: 2026-07-17
- Scope: DEV-405 repository implementation
- Decision: ADR-012
- Result: first-connection product loop and deterministic recovery guidance implemented and verified; external first-user observation remains a DEV-407 gate

## Implemented loop

| Step | Durable evidence | User action |
|---|---|---|
| Connect tool | compatible, authenticated `AdapterConnection` | install/repair the upstream CLI, complete its own login, Refresh |
| Trust workspace | `WorkspaceRecord.trusted` | add a canonical local root and explicitly trust it |
| Create sample | fixed marked `TaskRecord` | create one read-only workspace-inspection task |
| Queue and run | normal `QueueEntry` plus `RunSnapshot` | choose the first verified adapter; inspect or cancel in 2D |
| Human review | latest `TaskReviewRecord` | collect artifacts, record the handoff, then accept or return |

There is no onboarding table or local completion boolean. Relaunch reconstructs progress from normal product records. The sample creation command is idempotent per workspace and refuses untrusted workspaces.

## Recovery coverage

The main window provides concrete next actions for:

- missing CLI, failed version probe, unsupported Alpha version, unauthenticated/unknown auth;
- `path_policy_violation`, `unsupported_adapter_version`, `process_error`, and `adapter_error`;
- `store_error`, `internal_error`, `not_found`, `conflict`, and `invalid_argument`;
- `diagnostic_export_failed` and unknown runtime failures.

Recovery copy keeps authentication in the upstream terminal, preserves fail-closed state, and links metadata-only diagnostics without asking users to paste credentials or raw logs.

## Automated evidence

| Check | Result |
|---|---|
| Rust core | 53 tests passed, including trusted/idempotent sample creation |
| W1 probe guards | 2 tests passed |
| Frontend | 3 files / 17 tests passed |
| TypeScript | `pnpm typecheck` passed |
| macOS package contract | 3 Node tests and `pnpm release:check` passed without enabling automatic update |
| Rust formatting | `cargo fmt --all` and final `cargo fmt --all -- --check` passed |
| Windows Release regression | `pnpm desktop:build` produced `target/release/kruon-desktop.exe` with the new command and UI |
| Diff hygiene | `git diff --check` passed; temporary Cargo mirror configuration was removed |

## Honest boundaries

- The built-in task proves the same read-only control path in code; this verification did not launch a paid provider Run or modify a user workspace.
- A compatible binary can later lose network or provider access. Kruon reports the resulting Run failure and never upgrades it to completed.
- Completion still requires an explicit human review; repository automation cannot accept a user result.
- Usability comprehension, time-to-first-success, support burden, and 20–50 invite outcomes remain external DEV-407 evidence.
- DEV-406 requires an independent security reviewer; this implementation report is not that review.
