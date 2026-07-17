# ADR-011: macOS Alpha packaging and local-data lifecycle

- Status: Accepted with external release gates
- Date: 2026-07-17
- Scope: M4 DEV-404

## Context

Kruon previously built only a bare executable. The Tauri bundle was inactive, CI ran only on Linux, and no repository contract existed for a stable bundle identifier, macOS signing/notarization inputs, upgrade compatibility, or uninstall data handling. A release workflow must not embed Apple credentials or substitute placeholder updater keys, and Windows development cannot prove Gatekeeper or notarization behavior.

## Decision

1. `com.kruon.desktop` is the stable application identifier. `0.1.0` remains synchronized across Tauri, Cargo, and the desktop package before any Alpha package is produced.
2. Tauri bundling is active. The macOS Alpha script targets `aarch64-apple-darwin` and produces `.app` plus `.dmg`; macOS 12.0 is the frozen minimum for this Alpha contract.
3. Hardened Runtime is enabled. The reviewed entitlement file is an empty dictionary: Kruon adds no exception entitlement merely to make signing easier. Any future entitlement requires a security review and release-contract update.
4. Developer ID identity and notarization credentials come only from the `alpha-release` CI environment. They are not written to repository files, application data, logs, or artifacts.
5. `.github/workflows/macos-alpha.yml` is manual-only. It imports the certificate into an ephemeral keychain, builds/signs/notarizes/staples, verifies codesign, Gatekeeper assessment and stapling, uploads a private 14-day DMG artifact, and removes temporary signing material. It does not create a public GitHub Release.
6. App data remains under the OS application-data directory, outside the `.app` bundle and without an application-version path component. Upgrade keeps the same SQLite file and applies transactional schema migrations.
7. Uninstall retains local data by default. Removing retained history requires a separate explicit user action; Kruon does not silently delete it with the application bundle.
8. Application downgrade is fail-closed. If an older binary sees a `schema_migrations` version newer than its supported version, it refuses the database before WAL configuration, migration, or product writes.
9. Automatic update remains disabled. It may be enabled only after establishing updater-key custody, embedding the real public key, selecting an approved HTTPS endpoint/channel, generating signed updater artifacts, and testing install plus rollback on a clean macOS account. Placeholder keys/endpoints are forbidden.

## Consequences

- The repository can build and validate the package contract on Windows, while real Apple security evidence remains clearly separated.
- A normal macOS uninstall removes the application but keeps Kruon history. This favors data safety; a future settings flow must make deletion explicit and destructive.
- Rolling back only the application may be impossible after a forward schema migration. The older app refuses the newer database instead of guessing compatibility or corrupting it.
- Invite-only Alpha distribution is a manually triggered, private artifact workflow until release ownership, certificates, and updater keys are configured.

## Evidence

- `tauri.conf.json`, `entitlements.plist`, and package scripts define the frozen package contract.
- `scripts/check-macos-alpha-release.mjs` verifies version alignment, identifier, targets, icons, Hardened Runtime, empty entitlements, workflow presence, and signing/notarization environment names without printing values.
- `database.rs` rejects future schemas; `release.rs` owns stable OS app-data paths and the retain-by-default lifecycle policy.
- The Windows Release build proves the configuration parses and the no-bundle development artifact remains buildable.

## External release gates

- configure a paid Apple Developer team, Developer ID Application certificate, and `alpha-release` environment secrets;
- execute the manual workflow on macOS and retain codesign, Gatekeeper, notarization, and stapler evidence;
- install the DMG on a clean macOS 12+ user account, launch it, upgrade it over a prior Alpha, and confirm data preservation;
- uninstall the app and verify retained data plus the documented explicit-deletion procedure;
- design and verify the signed automatic-update channel before enabling `createUpdaterArtifacts` or the updater plugin.

## Revisit triggers

- changing bundle identifier, minimum macOS version, architecture, icon set, or entitlement set;
- adding App Store distribution, a public GitHub Release, or automatic publishing;
- adding an updater public key, endpoint, alternate channel, or rollback server;
- changing the SQLite location or uninstall retention default;
- adding a schema migration that requires destructive transformation or downgrade support.

## Primary references

- [Tauri macOS code signing and notarization](https://v2.tauri.app/distribute/sign/macos/)
- [Tauri macOS application bundle](https://v2.tauri.app/distribute/macos-application-bundle/)
- [Tauri updater signing and configuration](https://v2.tauri.app/plugin/updater/)
