# ADR-010: Metadata-only diagnostic export

- Status: Accepted
- Date: 2026-07-17
- Scope: M4 DEV-403

## Context

A support bundle is likely to be shared outside the local machine. Repackaging persisted events, task text, adapter output, or database files and then attempting to redact them would leave an unnecessarily large privacy surface: a missed token shape, custom secret, project name, prompt fragment, or absolute path could become irreversible once the file is shared.

## Decision

1. Kruon constructs diagnostics from a metadata allowlist. It does not copy SQLite, event payloads, artifacts, prompts, task text, workspace records, adapter command output, or raw logs into the bundle.
2. The bundle contains only application/platform versions, applied database schema versions, aggregate record counts, sanitized adapter compatibility state, and anonymous Run summaries.
3. A Run summary uses a bundle-local ordinal instead of a Run ID. It may include adapter, status, terminal state, event count, phase counts, last sequence, and the bounded numeric/boolean terminal-output diagnostics established by ADR-009.
4. Exact versions are included only after strict three-component numeric normalization. Raw version output, executable command/path, probe detail, capabilities, and authentication material are excluded.
5. At most the latest 50 Run summaries are included. The bundle records total/included counts and whether summaries were truncated.
6. Before serialization reaches disk, a second validation rejects forbidden field names, known secret shapes, and Windows drive, UNC, macOS, or Linux absolute paths. A validation failure produces no final bundle.
7. Exports are capped at 1 MiB and written atomically through a same-directory temporary file, `sync_all`, and rename. A failed write removes the temporary file.
8. The main 2D window may request export. The world-view capability does not receive the command. The frontend receives only a file name, location class, size, checksum, and counts—not an absolute destination path.
9. The default destination is the user's Downloads directory; Kruon app data is a fallback when the OS cannot resolve Downloads. Export never uploads or transmits the file.

## Consequences

- The bundle is intentionally less detailed than a raw log archive. Support can diagnose compatibility, schema, lifecycle, and output-containment state without receiving user content.
- Custom, nonstandard secrets cannot be exhaustively recognized by patterns. The primary control is that user-controlled strings never enter the diagnostic schema; the secondary scan is defense in depth.
- A maximum of 50 anonymous Run summaries keeps export work and file size bounded.
- A user must deliberately share the generated JSON file; Kruon has no upload endpoint in this workflow.

## Evidence

- `diagnostics.rs` owns the allowlisted schema, cross-platform scanner, size cap, atomic writer, and privacy regression tests.
- `runtime.rs` collects only the latest bounded Run/event metadata and schema versions.
- `lib.rs`, the Tauri command manifest, and `main-control-commands.toml` expose export only to the main control window.
- `App.tsx` shows the exclusions before export and reports only the non-sensitive export receipt.

## Revisit triggers

- support evidence shows the metadata-only bundle cannot diagnose a high-frequency Alpha failure;
- adding a new string field to the diagnostic schema;
- adding attachments, logs, crash dumps, database excerpts, or automatic upload;
- changing the export destination or introducing a file-picker/plugin permission;
- adding a new platform whose absolute-path syntax is not covered by the secondary scan.
