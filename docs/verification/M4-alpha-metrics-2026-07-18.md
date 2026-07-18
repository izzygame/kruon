# M4 consented Alpha metrics verification

- Date: 2026-07-18
- Scope: DEV-407 repository-side measurement and support preparation
- Decision: ADR-014
- Result: local aggregate dashboard/export implemented; real 20–50 user recruitment, support evidence, and decision remain open

## Implemented contract

- The main control window displays on-device task/Run totals, terminal success, and review coverage without adding a telemetry client.
- Export requires a fresh unchecked-by-default consent checkbox. A successful export resets it; a missing consent is rejected in both the UI and Rust command path.
- The JSON snapshot contains only aggregate readiness, onboarding stages, counts, timing, denominator-defined rates, app/platform metadata, and explicit privacy flags.
- No install/participant/workspace/task/queue/Run/review identifiers, names, prompt or task text, file locations, event payloads, process output, credentials, or raw records are serialized.
- The existing diagnostic privacy validator scans the final JSON value, the body is capped at 64 KiB, and disk output is create-new plus atomic rename. Kruon does not upload it.
- The main-window capability includes only the explicit export command; the world window does not receive it.

## Automated evidence

| Check | Result |
|---|---|
| Rust aggregate privacy fixture | passed: sensitive identities, text, paths, hashes, process metadata, and review notes did not appear |
| Rust consent/write fixture | passed: denied consent wrote no file; approved export wrote one bounded local file |
| Rust all targets | 59 passed: 57 core/library tests plus 2 probe guards |
| Frontend consent flow | passed: no invoke before consent, one explicit invoke after consent, result shown, checkbox reset |
| Frontend suite and TypeScript | 3 files / 19 tests passed; `pnpm typecheck` passed |
| Security drift gate | 4 mutation/contract tests passed; removing the generated command permission from the main capability fails closed |
| Release/package contract | 3 tests plus `pnpm release:check` passed |
| Windows Release build | `pnpm desktop:build` produced `target/release/kruon-desktop.exe` (11,601,408 bytes) |
| Formatting/diff hygiene | final checks passed; temporary Cargo mirror configuration removed |

During implementation the shared privacy validator rejected an export field whose name contained forbidden diagnostic terms. The export failed closed and wrote no file. The field was replaced with non-sensitive positive inclusion flags, then both aggregate-export tests and the full Rust target suite passed. This is expected guard behavior, not participant evidence.

## Manual product check

1. Open the main control window and locate `DEV-407 · local evidence`.
2. Confirm the export button is disabled and the consent checkbox is clear.
3. Confirm the copy says the snapshot stays on this device and excludes participant identity, workspace/task text, paths, prompts, logs/events, and automatic upload.
4. Select consent, export once, and confirm the checkbox clears after success.
5. Inspect the JSON before sharing; confirm it contains aggregate fields only.

## External evidence still required

- Recruit and consent 20–50 target users through the operator-owned process.
- Observe first use and collect usability/comprehension evidence without committing participant data to this repository.
- Operate P0–P3 support triage and report denominator-backed aggregate outcomes.
- Complete DEV-404 release gates and DEV-406 independent security review.
- Record a Go/Pivot/No-Go decision with limitations and unresolved risks.

Repository implementation does not satisfy those gates and must not be reported as a completed invited Alpha.
