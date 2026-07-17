# M4 external security review packet

- Prepared: 2026-07-17
- Scope: DEV-406 independent review handoff
- Internal decision: ADR-013
- External status: **not yet reviewed**
- Review target: **must be filled with a frozen commit/tag before handoff; the current development worktree is not a review identifier**

## Reviewer objective

Determine whether the invite-only, read-only Alpha can safely launch the frozen Codex and Claude Code adapters without violating Kruon's stated workspace, credential, event-integrity, diagnostic, capability, cancellation, and local-data boundaries. Report critical/high findings with a reproducible proof and identify assumptions that are not enforceable by the current architecture.

This packet does not ask the reviewer to certify network or OS isolation that the product does not implement.

## In-scope trust boundaries

1. Tauri main control window versus the projection-only world window.
2. Rust runtime versus the untrusted third-party CLI process and its structured output.
3. Explicitly trusted canonical workspace versus the rest of the filesystem.
4. Local SQLite/event/audit/diagnostic data versus other local users and exported support files.
5. Bundled application, resolved per-user CLI shim/binary, Node runtime, and upstream authentication state.
6. Manual macOS signing/notarization workflow and retained application data across upgrade/uninstall.

## Highest-priority attack scenarios

- escape read-only/workspace bounds through symlink replacement, rename races, path casing/normalization, junctions, or artifact/completion-report paths;
- spoof a supported CLI version with a replaced shim/binary and bypass the fixed launch plan;
- inject commands through task text, adapter JSON, malformed/oversized/invalid UTF-8 output, executable path, environment, or Tauri IPC;
- obtain a process, filesystem, credential, diagnostic-export, cancel, approval, or enqueue capability from the world window;
- leak secrets, prompts, project identity, absolute paths, raw logs, auth status output, or environment values into SQLite/UI/diagnostic files;
- race cancel/wait/finalize/restart/review paths into a false `completed`, duplicate terminal, stale approval, or auto-accepted state;
- substitute or cross-user read the SQLite/WAL/SHM files; attempt a newer-schema downgrade or partial migration;
- use unrestricted CLI network access or process-group escape to exceed the product's honest security claims;
- compromise the signed-update/release supply chain or persist signing material after CI completion.

## Repository controls to verify

| Control | Primary evidence |
|---|---|
| Fixed read-only adapter arguments; prompt on stdin | `core/adapter_host.rs` plus launch-plan tests |
| Sanitized child/probe environment | `adapter_host.rs`, `process_supervisor.rs`, `m1.rs` |
| Exact version compatibility gate | `core/m4.rs`, compatibility fixtures, ADR-008 |
| Workspace trust, canonical scope, artifact/report path checks | `m1.rs`, `path_policy.rs`, `runtime.rs` |
| Trust revocation and launch-time recheck | `runtime.rs`, Tauri main capability, `App.tsx` |
| Event append/terminal integrity and crash recovery | `event_store.rs`, `runtime.rs` |
| Bounded output and malformed input handling | `process_supervisor.rs`, `adapter_host.rs` |
| No-follow/private local store and fail-closed schema | `database.rs`, `release.rs` |
| Recursive secret redaction and metadata-only diagnostics | `adapter_host.rs`, `diagnostics.rs` |
| World capability isolation and CSP | `capabilities/`, `permissions/`, `tauri.conf.json` |
| Human-only acceptance | `m2.rs`, main UI, M2 verification |
| Signed/notarized package contract | ADR-011, release workflow and preflight script |

## Reproduction commands

Run from a clean checkout of the frozen review target:

```powershell
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test
pnpm security:check
node --test scripts/check-alpha-security.test.mjs
node --test scripts/check-macos-alpha-release.test.mjs
pnpm release:check
pnpm audit --prod --audit-level high --registry=https://registry.npmjs.org
Push-Location apps/desktop/src-tauri
cargo fmt --all -- --check
cargo test --all-targets -- --test-threads=1
Pop-Location
pnpm desktop:build
```

The reviewer should additionally run a pinned RustSec audit, macOS static/dynamic inspection, and signed clean-user installation. `cargo-audit` was not installed in the preparation environment and therefore has no local result in this packet.

## Current risk register

| ID | Severity | Status | Risk / required disposition |
|---|---|---|---|
| M4-S01 | High | Open | CLI network egress is unrestricted. Do not claim full isolation; assess domain/process egress controls before write mode or public release. |
| M4-S02 | High | Open | No OS filesystem sandbox; canonicalization still has runtime TOCTOU and Unix descendants may escape a process group. Keep adapters read-only and reassess Seatbelt/current macOS mechanisms. |
| M4-S03 | High | Open | Supported version output is not executable provenance. Review code-signature/publisher/hash strategies and the usability cost of enforcing them. |
| M4-S04 | High | Mitigated, external verification pending | SQLite target no-follow plus Unix `0700/0600` is implemented. Verify WAL/SHM modes and symlink/junction behavior on clean macOS and Windows accounts. |
| M4-S05 | Medium | Mitigated | Trust revocation blocks new/queued launch; active Runs require explicit cancellation. Review revocation/dispatch races. |
| M4-S06 | High | Mitigated | World window remains projection-only and direct start is not registered; mutation-based CI gate prevents common capability/CSP drift. Attempt Tauri navigation/IPC bypasses. |
| M4-S07 | High | Mitigated, residual open | Recursive redaction and diagnostic allowlists are tested, but unknown secret formats remain possible. Fuzz structured events and output encodings. |
| M4-S08 | Medium | Partial | Official npm production audit reported no known advisories on 2026-07-17. Pinned RustSec and transitive license/supply-chain checks remain required. |
| M4-S09 | High | External gate | Real Developer ID signing, notarization, Gatekeeper/stapling, clean upgrade/uninstall, and updater key custody remain DEV-404 gates. |

## Requested deliverables

- methodology, tooling, review target commit and environment;
- findings rated Critical/High/Medium/Low with CWE where applicable;
- minimal reproduction and affected boundary for each finding;
- distinction between exploitable defect, defense-in-depth gap, and documented product limitation;
- recommended fix and validation method;
- explicit retest result for every Critical/High finding;
- residual-risk statement and release recommendation for invite-only read-only Alpha only.

DEV-406 closes only after the review target is frozen, an independent reviewer delivers this evidence, and every Critical/High item is fixed or explicitly rejected by the product/security owner with rationale. Kruon contributors must not self-approve that gate.
