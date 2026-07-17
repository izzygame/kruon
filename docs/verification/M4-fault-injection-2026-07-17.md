# M4 fault injection and recovery verification

- Date: 2026-07-17
- Scope: DEV-402
- Decision: ADR-009
- Result: implementation and local automated evidence verified; later M4 repository gates are tracked separately and M4 remains open for DEV-404 external gates, DEV-406 independent review, and DEV-407 user evidence

## Fault matrix

| Fault | Injection | Required invariant | Result |
|---|---|---|---|
| Kruon restart with an active Run | reopen a temporary database containing a nonterminal Run | append one `run.recovery_uncertain`; terminal is `unknown`, never `completed`; second recovery is idempotent | passed |
| Agent crash | Windows `cmd.exe` fixture exits with code 7; cross-platform runtime outcome fixture | Run is `failed`, exit code preserved, output captured | passed |
| Invalid UTF-8 / garbled output | byte stream contains invalid bytes between two valid lines | replacement decoding is counted and the later terminal-shaped line is still captured | passed |
| Oversized line | line exceeds 256 KiB, followed by another line | first line bounded, truncation recorded, following line drained | passed |
| Excess line count | 10,001-line stream | at most 10,000 lines retained and truncation recorded | passed |
| Timeout | Windows child exceeds 100 ms; runtime timeout outcome fixture | cancellation/uncertain truth wins; never `completed` | passed |
| Disk full | temporary SQLite database freezes `max_page_count`, then receives an 8 MiB event | event insert and Run projection both roll back | passed |
| Legacy migration | temporary version-1 schema without `launch_fingerprint` | Run survives, missing field gains safe default, versions 1-2 recorded | passed |
| Full product schema | open RuntimeCore on a new temporary database | transactional versions 1, 2, 3, and 4 are present | passed |
| Upgrade failure | inject an error after transactional DDL/data writes but before commit | created table and partial data do not exist after rollback | passed |

All database tests use disposable temporary or in-memory databases. The real Kruon database and real disk capacity were not modified.

## Output containment

| Limit | Frozen value |
|---|---:|
| Captured bytes per stdout/stderr stream | 4 MiB |
| Captured lines per stream | 10,000 |
| Captured bytes per line | 256 KiB |

Readers continue consuming bytes after a limit is reached to avoid deadlocking a child on a full pipe. Runtime terminal events include only `stdout_truncated`, `stderr_truncated`, `stdout_lossy_lines`, `stderr_lossy_lines`, and retained line counts. Dropped bytes and raw invalid output are not stored.

## Database guarantees

- Event insertion and the matching Run projection update share one transaction.
- EventStore, M1 control, and M2 review migrations use the same transactional wrapper.
- A failed migration leaves no partial DDL or data from that migration.
- A legacy Run survives the version-2 column migration with an empty launch fingerprint rather than being deleted or rewritten.
- If storage remains full, Kruon can fail initialization or leave a Run nonterminal; it must not invent completion. After storage is restored, restart recovery can conservatively close the Run as `unknown`.

## Automated evidence

| Check | Result |
|---|---|
| Rust core | 46 tests passed on Windows |
| W1 probe guards | 2 tests passed |
| Windows native process faults | nonzero exit and timeout cases present in and executed by the Windows test binary |
| Frontend | no contract change; existing frontend suite remains required in the final release regression |

## Known non-claims

- `SQLITE_FULL` is induced through SQLite's page ceiling; the machine's actual disk is not filled.
- App restart is modeled through persisted nonterminal state and RuntimeCore reopen, not by forcibly killing the user's live Kruon window.
- Database transaction rollback does not prove macOS signature, notarization, installer or automatic-update rollback. DEV-404 now rejects newer schemas in older apps, but real Apple and clean-machine evidence remains open.
- The current Windows process-tree path uses `taskkill /T`; a native Job Object remains a Windows Beta hardening item.
