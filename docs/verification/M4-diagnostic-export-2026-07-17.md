# M4 metadata-only diagnostic export verification

- Date: 2026-07-17
- Scope: DEV-403
- Decision: ADR-010
- Result: implementation and local automated evidence verified; later M4 repository gates are tracked separately and M4 remains open for DEV-404 external gates, DEV-406 independent review, and DEV-407 user evidence

## Frozen bundle contract

| Area | Included | Explicitly excluded |
|---|---|---|
| Product/runtime | app version, target OS, target architecture, diagnostic schema version | machine name, user name, environment, absolute destination path |
| Database | applied schema version numbers and aggregate record counts | SQLite file, database location, row contents |
| Connections | adapter kind, status, strict normalized version, compatibility, authentication classification | executable command/path, raw version output, probe detail, capability text, credentials |
| Runs | bundle-local ordinal, adapter, lifecycle/terminal state, event/phase counts, last sequence | Run ID, PID/PGID, policy ID, hashes, fingerprints, workspace/project references |
| Terminal health | exit code, forced-stop/residual flags, line counts, truncation and lossy-line counts | event payloads, stdout/stderr text, reasons, logs |
| Tasks/artifacts | counts only where applicable | title, goal, context, prompt, acceptance/test/rollback text, allowed/changed paths, artifact/review content |

Only the latest 50 Run summaries are eligible. The bundle records total and included counts so truncation is visible. The serialized JSON is limited to 1 MiB.

## Privacy and write gates

1. Rust types construct the export from the allowlist; user-controlled strings do not have a field through which to enter the serialized bundle.
2. A second recursive scan rejects forbidden field names, common secret shapes, Windows drive paths, UNC paths, and Unix/macOS absolute paths.
3. Strict semantic versions are the only free-standing external strings admitted from adapter discovery.
4. Serialization and validation finish before a file is created.
5. The writer uses a unique same-directory temporary file, flushes it with `sync_all`, renames atomically, and removes the temporary file after failure.
6. Downloads is the default destination. App data is a fallback; the UI receipt exposes only the location class and file name.
7. `export_diagnostic_bundle` belongs to `main-control-commands`; `world-readonly` cannot invoke it.

## Automated evidence

| Check | Result |
|---|---|
| Sensitive-source fixture | seeded token, prompt, project name, Windows path, raw log, IDs, hashes, fingerprint, command detail absent from output |
| Secondary scanner | forbidden key, Bearer secret, Windows path, and macOS path rejected |
| Atomic export | one bounded JSON file created; checksum/size match; no temporary file remains |
| Rust core | 49 tests passed on Windows |
| W1 probe guards | 2 tests passed |
| Frontend | 3 files / 12 tests passed, including export command and privacy copy |
| TypeScript | `pnpm typecheck` passed |
| Formatting/diff | `cargo fmt --all -- --check` and `git diff --check` passed |
| Release | `pnpm desktop:build` produced the Windows executable |

## Known non-claims

- The bundle is JSON, not a raw log archive or database backup.
- No diagnostic file is uploaded, emailed, or otherwise transmitted automatically.
- Pattern matching cannot identify every arbitrary custom secret. Privacy primarily comes from the typed metadata allowlist; the scanner is a second gate.
- The successful Windows Release build does not satisfy macOS signing, notarization, installer or automatic-update evidence. DEV-404 now has a separate repository preflight, while its Apple and clean-machine gates remain open.
