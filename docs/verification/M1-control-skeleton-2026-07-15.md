# M1 Control Skeleton acceptance record

- Date: 2026-07-15
- Scope: M1 / DEV-101 through DEV-108 in `kruon_MVP开发计划_2026-07-11.md`
- Result: implementation complete; Windows build and platform-neutral tests pass. The production macOS target still requires its normal CI/device run for the Unix process-tree test.

## Delivered behavior

| M1 item | Delivered implementation |
| --- | --- |
| DEV-101 | The console probes Codex and Claude Code on a controlled environment, then shows command discovery, version, conservative authentication state, approval mode, and fixed read-only capabilities. |
| DEV-102 | Workspace roots are canonicalized. A workspace starts untrusted and must be explicitly trusted before a noninteractive CLI Run can be enqueued. Task scopes are normalized and checked inside the canonical root. |
| DEV-103 | The task form persists title, goal, context, allowed paths, acceptance criteria, test plan, and rollback plan. The generated adapter prompt is explicitly read-only. |
| DEV-104 | Queue entries are durable in SQLite. A serialized dispatcher starts at most two non-terminal Runs and accepts a manual Codex or Claude selection for each task. |
| DEV-105 | The existing Process Supervisor and fixed Adapter Host remain the only launch path. Windows now uses process-tree termination through `taskkill`; Unix keeps process-group signaling. |
| DEV-106 | Workspace, task, queue, Run, and normalized event projections are exposed through Tauri commands and rendered in a 2D console board. |
| DEV-107 | The board shows normalized event sequence metadata only. Existing adapter normalization/redaction continues to prevent raw prompts, fixture text, and secret-shaped fields from being persisted or surfaced. |
| DEV-108 | Store tests cover persistence/restart reservation recovery and untrusted launch rejection. Unix fixture coverage adds a three-Run, two-slot dispatcher test with isolated event streams. |

## Exit-gate evidence

| Exit gate | Evidence |
| --- | --- |
| Connect two tools, create a Workspace and Task | The Tauri UI has a refreshable two-adapter connection panel plus Workspace and Task forms backed by `probe_connections`, `create_workspace`, and `create_task`. Four UI tests verify the connection display and workspace request shape. |
| Two isolated Runs without status mixing | The dispatcher uses `EventStore::active_run_count()` and `MAX_CONCURRENT_RUNS = 2`. The Unix fixture test queues three Codex Runs, verifies two active/one queued, then verifies all event streams retain their own Run ID. |
| Restart restores queue, Runs, and unknown status | `EventStore::recover_interrupted_runs()` projects non-terminal Runs to `unknown`. Bound queue entries keep their Run ID; only a reservation that had no bound Run is requeued. The control-store persistence test reopens its SQLite database and confirms Workspace, Task, and Queue state. |
| Untrusted directory cannot launch CLI | `RuntimeCore::enqueue_task_run()` rejects an untrusted Workspace before it creates a queue entry or Run. The platform-neutral Rust test asserts both stores stay empty. UI Run buttons are disabled until trust is explicit. |

## Verification performed on this Windows workstation

```text
pnpm typecheck                         PASS
pnpm test                              PASS (4 UI tests)
pnpm build                             PASS
cargo fmt                              PASS
cargo test --all-targets               PASS (19 tests)
cargo check --all-targets              PASS
```

`cargo test --all-targets` on Windows intentionally excludes the Unix process-group fixture module. The same module is compiled and run on the macOS delivery target; it is the final target-specific verification needed before a macOS release artifact is declared verified.

## Deliberate M1 boundary

This stage does not add M2 approval decisions, artifact acceptance, write-capable execution, cloud credential configuration, local model controls, or 3D interaction. The console remains a local, read-only planning control surface.
