# M4 macOS Alpha release preflight

- Date: 2026-07-17
- Scope: DEV-404 repository implementation
- Decision: ADR-011
- Result: repository packaging and data-lifecycle controls verified on Windows; signed/notarized macOS package, clean-machine lifecycle, and automatic update remain external gates

## Implemented contract

| Area | Repository state | Current evidence |
|---|---|---|
| Bundle identity/version | `com.kruon.desktop`; Tauri/Cargo/desktop versions synchronized | Node release-contract tests |
| Target package | Apple Silicon `.app` + `.dmg`; macOS 12.0 minimum | Tauri config and frozen package script |
| Runtime signing posture | Hardened Runtime on; empty entitlement dictionary | release preflight rejects drift or added entitlement keys |
| Signing/notarization inputs | CI environment only; no values printed or stored | secret-name gate test and manual workflow |
| CI key handling | ephemeral keychain/certificate, cleanup under `always()` | workflow review; execution pending configured secrets |
| Package verification | codesign, Gatekeeper and stapler commands are mandatory after build | workflow review; macOS execution pending |
| Application data | stable OS app-data path, outside app bundle, no version component | Rust lifecycle tests |
| Upgrade | same SQLite location plus transactional migrations | existing migration suite and schema 1-4 evidence |
| Application downgrade | future schema rejected before configuration/migration/write | new future-schema mutation guard test |
| Uninstall | application removal retains data by default | frozen code/document policy; clean-user execution pending |
| Automatic update | disabled fail-closed | no updater plugin, endpoint, public key, or updater artifacts |

## Automated evidence

| Check | Result |
|---|---|
| Rust core | 52 tests passed on Windows |
| W1 probe guards | 2 tests passed |
| Release contract | 3 Node tests passed |
| Repository package gate | `pnpm release:check` passed |
| Frontend | 3 files / 12 tests passed |
| TypeScript | `pnpm typecheck` passed |
| Windows Release regression | `pnpm desktop:build` produced the executable with the updated Tauri configuration |
| Formatting/diff | `cargo fmt --all -- --check` and `git diff --check` passed |

The local Windows build uses `--no-bundle`; it validates the shared Tauri configuration and executable regression, not `.app` or `.dmg` creation.

## Manual macOS workflow contract

The `macOS Alpha package` workflow is `workflow_dispatch` only and requires the protected `alpha-release` environment. It expects:

- `APPLE_CERTIFICATE`
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_SIGNING_IDENTITY`
- `APPLE_ID`
- `APPLE_PASSWORD`
- `APPLE_TEAM_ID`

The preflight also accepts Tauri's App Store Connect API credential family, but the checked-in workflow currently wires the Apple ID family. Secret values are never echoed by the preflight.

## Open evidence gates

- No Developer ID certificate or Apple notarization credential was available in this Windows workspace.
- The macOS workflow has not been run; `.app`, `.dmg`, codesign, Gatekeeper, notarization and stapling evidence do not yet exist.
- No clean macOS user install/launch/upgrade/uninstall cycle has been executed.
- Automatic update is deliberately not scaffolded with dummy cryptographic material. It still needs a real Tauri updater keypair custody decision, public key, HTTPS endpoint, signed artifacts, user-facing consent flow, and rollback test.
- Current native icons are still technical Alpha placeholders pending approved brand export.

Therefore DEV-404 is materially implemented but not closed. It may close only after the external evidence gates above pass.
