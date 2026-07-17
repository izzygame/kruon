# M4 Alpha compatibility verification

- Date: 2026-07-16
- Scope: DEV-401
- Decision: ADR-008
- Result: implementation verified; later M4 repository gates are verified separately; M4 remains open for DEV-404 external gates, DEV-406 independent review, and DEV-407 user evidence

## Alpha matrix

| Adapter | Allowed patch versions | Evidence boundary |
|---|---|---|
| Codex | `0.144.1`, `0.144.2` | one checked-in structured fixture per patch; `0.144.2` also has the W1 controlled read-only probe record |
| Claude Code | `2.1.205`, `2.1.211` | one checked-in structured fixture per patch; `2.1.205` also has the W1 controlled read-only probe record |

The matrix is intentionally exact. A different patch, an unparseable result, or a prerelease result is not silently treated as compatible.

## Implemented execution path

1. Resolve the real per-user CLI before inherited `PATH` fallbacks.
2. Run the existing detached, sanitized version probe.
3. Extract a stable three-part version from product-shaped output such as `codex-cli-exec 0.144.1` or `2.1.211 (Claude Code)`.
4. Compare it with the adapter's exact Alpha allowlist.
5. Return `ready` only for a supported version; otherwise return `unsupported_version` or `version_check_failed` and skip authentication classification.
6. Disable that adapter's Run button in 2D and explain the blocked matrix in the connection card.
7. Recheck compatibility in Rust immediately before Run creation. A forged or stale frontend request cannot bypass the gate.

Successful probe output uses stdout first and falls back to stderr only when stdout is empty, which covers Codex's successful login-status shape. Claude Code's `loggedIn` JSON boolean is parsed directly. Only the normalized authentication enum reaches the UI; account fields and raw status output are not persisted or displayed.

Existing Runs and event history remain available when a newly installed version is blocked.

## Fixture regression

`apps/desktop/src-tauri/fixtures/compatibility/` contains four JSONL fixtures. The M4 invariant test requires every allowlisted version to have exactly addressable fixture evidence, normalizes every line through the production `AdapterHost`, requires all event types to be known, verifies a final `completed` terminal state, and proves embedded fixture secrets are removed.

## Local evidence

The local commands resolved from `C:\Users\Izzy\AppData\Roaming\npm` reported:

- Codex: `codex-cli-exec 0.144.1`
- Claude Code: `2.1.211 (Claude Code)`

Both are in the frozen matrix. The release desktop showed both connection cards as `ready` and `supported`, displayed the exact Alpha allowlists, and left the existing Run controls governed by the same connection state. Opening the optional world window still loaded the packaged Kenney office after its lazy-load interval, with both no-Run stations projected as `sleeping`.

No provider Run or user Workspace was created during this pass. This version check does not claim that provider authentication or network availability can never change.

## Automated evidence

| Check | Result |
|---|---|
| Rust all-target tests | 34 core tests + 2 probe guard tests passed |
| M4 compatibility tests | parser rejection cases and four matrix fixtures passed inside the Rust suite |
| Frontend tests | 3 files, 11 tests passed, including unsupported-version button blocking |
| TypeScript | `tsc --noEmit` passed |
| Rust formatting | `cargo fmt --all -- --check` passed after formatting |
| Production frontend | Vite production build passed; world renderer and Kenney asset layers remain separate lazy chunks |
| Windows release executable | `tauri build --no-bundle` passed; connection matrix and world-window regression verified live |

## Remaining M4 gates

- DEV-402: verified in [M4 fault injection and recovery](M4-fault-injection-2026-07-17.md);
- DEV-403: verified separately with a metadata allowlist plus secret, prompt, project-name, and full-path exclusion tests;
- DEV-404: repository packaging/data policy implemented; real macOS signing, notarization, clean install/upgrade/uninstall, and automatic update remain external gates;
- DEV-405: repository implementation verified in the onboarding report;
- DEV-406: internal preflight and review packet complete; independent external review remains open;
- DEV-407: invited Alpha recruitment, support, and consented product metrics.

M3 DEV-307 and the earlier W1 interview gate also remain external-evidence items. DEV-401 does not close those research gates.
