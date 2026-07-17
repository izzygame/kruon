# ADR-009: Alpha fault containment and migration atomicity

- Status: Accepted
- Date: 2026-07-17
- Scope: M4 DEV-402

## Context

Agent processes and SQLite can fail independently of the frontend. Unbounded output can exhaust memory, invalid UTF-8 can stop a text reader before later terminal events, a nonzero exit can be mistaken for successful completion, and a partial schema migration can leave the local database unusable. Alpha must fail closed without testing against or filling a user's real disk.

## Decision

1. Process output is read as bytes rather than UTF-8 lines. Invalid sequences are converted lossily for normalization, counted, and do not stop later output from being drained.
2. Each stdout/stderr stream is limited to 256 KiB per line, 4 MiB total captured bytes, and 10,000 captured lines. The reader continues draining after a limit so the child process cannot block on a full pipe.
3. Truncation and lossy-line counts are written only as normalized terminal diagnostics. Raw stderr, invalid bytes, and dropped output are not persisted.
4. Nonzero exits remain `failed`; timeout cancellation remains `cancelled` or `uncertain` if cleanup cannot be proven. Neither path may become `completed` because an adapter emitted a source-level terminal event.
5. Event, M1 control, and M2 review schemas migrate inside SQLite transactions and register schema versions 1 through 4.
6. Event insertion and Run projection updates remain one transaction. `SQLITE_FULL` or another store error must leave both unchanged.
7. A migration error rolls back the whole migration transaction. This is database migration rollback, not signed application-package downgrade, which remains under DEV-404.

## Consequences

- Very large adapter events may become a hashed malformed event rather than a complete structured event. The terminal diagnostic makes truncation visible without storing the raw content.
- Output memory is bounded to approximately 8 MiB of captured payload across stdout and stderr per Run, plus collection overhead.
- A disk-full process can remain nonterminal until storage is available and restart recovery can record `unknown`; it is never reported as successful.
- Exact schema versions make migration state inspectable, while transaction rollback prevents half-applied DDL.

## Evidence

- `process_supervisor.rs` owns byte capture, limits, Windows crash/timeout tests, and diagnostic flags.
- `database.rs` owns the shared transactional migration wrapper and injected rollback test.
- `event_store.rs` covers legacy upgrade preservation and real SQLite max-page `SQLITE_FULL` rollback.
- `runtime.rs` covers crash/timeout terminal truth and persists only bounded-output metadata.

## Revisit triggers

- observed valid adapter events exceed the frozen line or stream budgets;
- introducing streaming persistence instead of outcome-time event normalization;
- adding a schema migration that cannot run inside a SQLite transaction;
- enabling automatic application updates or database downgrade tooling;
- moving from a single local SQLite writer to a writer queue or service process.
