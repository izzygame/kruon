# M3 minimal world view: plan and verification

- Date: 2026-07-16
- Scope: DEV-301 through DEV-307
- Decision: ADR-007
- Product boundary: optional read-only projection; the 2D control path remains authoritative

## Delivery plan

1. Freeze the projection contract and eight state mappings in Rust, sourced only from persisted Run/Event and review records.
2. Create a separate `world` webview and replace Tauri's implicit custom-command access with an explicit application manifest and per-window capabilities.
3. Add a fixed low-poly two-desk office, lazy-load its renderer, render on demand, and destroy it when the world window closes.
4. Make desk/card selection focus the same Run in the main 2D view without mutating the Run.
5. Verify projection consistency, permission boundaries, 2D independence, frontend behavior, production chunking, and live desktop behavior.
6. Run DEV-307 with recruited users before treating the M3 product gate as fully accepted.

## Frozen state projection

| Run/Event truth | World state |
|---|---|
| no Run for adapter | `sleeping` |
| pending | `idle` |
| planning, setup | `planning` |
| running, tool call, artifact, approval decision | `running` |
| waiting approval | `waiting_approval` |
| cancelling, degraded, failed, cancelled, forced stop, uncertain, returned review | `blocked` |
| completed without human review | `reviewing` |
| accepted completed Run | `completed` |

Each station carries `runId`, `runStatus`, `sourceSequence`, and `updatedAt`; it carries no prompt, workspace path, PID, raw event payload, artifact content, approval parameters, or credentials.

## Scene and power budget

| Budget | Frozen value |
|---|---:|
| Stations / agents | 2 |
| Meshes / geometries / materials | at most 32 each |
| Procedural fallback textures / external models | 0 |
| M3.1 selected visual layer | 8 GLB files + 1 shared texture; 606,458 bytes |
| M3.1 imported visible geometry | approximately 3,581 triangles |
| M3.1 texture ceiling | one 512 x 512 indexed PNG |
| Device pixel ratio | at most 1.25 |
| Visible polling | 2 seconds |
| Hidden polling | 15 seconds |
| Frame loop | `demand` |
| GPU preference | `low-power` |

The structural budget is covered by frontend tests. The following release-build baseline was measured on `DRAGON` with 16 logical processors. It is a local regression baseline, not a cross-device guarantee.

| Release observation | 2D only | 2D + world |
|---|---:|---:|
| Process tree | 7 processes | 8 processes |
| Working set | 413.9 MiB | 489.0-497.1 MiB |
| Private memory | 259.8 MiB | 298.7-306.9 MiB |
| CPU over a 5-second sample, normalized to total logical-CPU capacity | 0.76% | 0.57-0.98% |

Opening the world added 75.1-83.2 MiB of working set and 38.9-47.1 MiB of private memory in this run. Five-second CPU samples were noisy and did not establish a meaningful incremental CPU cost; use a longer trace before freezing a cross-device CPU ceiling.

## External asset implementation

M3.1 keeps the procedural geometry scene as the permanent safe fallback and adds a selected Kenney visual layer. Kenney's asset-page downloads are CC0 and do not require attribution. The selected sources are [Furniture Kit](https://kenney.nl/assets/furniture-kit) for the office and [Mini Characters](https://kenney.nl/assets/mini-characters) for agents.

Neither complete pack is vendored. The implemented import passes these gates:

- select only the models visible in the two-station office and lazy-load their upstream GLB files only inside the `world` window;
- preserve the current procedural scene as the renderer-error and missing-asset fallback;
- keep the initial compressed world-asset payload at or below 5 MiB, visible geometry at or below 50k triangles, and textures at or below 512 px per axis;
- sample the bundled `idle` clip once to avoid a bind pose; no continuous animation loop runs, and the eight authoritative states continue to come from color, status text, and the Rust projection;
- add a repository asset manifest recording source URL, downloaded pack version/date, original license file, selected files, conversions, and hashes;
- load no model, texture, font, or license data from a remote CDN at runtime.

ADR-007 is amended with the selected-asset allowlist, local-only CSP boundary, budget, and procedural fallback. See [M3.1 Kenney verification](M3.1-kenney-assets-2026-07-16.md) for the exact inventory and release measurements.

## DEV acceptance matrix

| ID | Implementation evidence | Status |
|---|---|---|
| DEV-301 | `WorldScene.tsx`, `assets.ts`, asset manifest; fixed two-desk scene, selected Kenney layer, procedural fallback, tested payload/triangle budgets | verified |
| DEV-302 | Rust `m3.rs`; all eight states covered by unit tests | implemented |
| DEV-303 | `focus_main_run` validates the Run, emits selection to `main`, and focuses 2D | implemented |
| DEV-304 | async dynamic Tauri world window, two-stage lazy imports, demand rendering, hidden polling, render error boundary; close-isolation verified in release build | verified |
| DEV-305 | explicit build command manifest plus `main-control` and `world-readonly` capabilities | implemented |
| DEV-306 | projection preserves source sequence/terminal truth; UI snapshot/focus tests | implemented |
| DEV-307 | local test protocol defined below; recruited participant results | external evidence open |

## Live desktop verification

- Release EXE loaded the existing local database and reported Codex `0.144.1` and Claude Code `2.1.211` as ready.
- `Open world view` created a separate `world` window and rendered the two-desk scene. With no local Runs, both stations correctly projected `sleeping`.
- Closing the world window destroyed that WebView while the main 2D window remained responsive and authoritative; reopening created a fresh world window and rendered the same projection again.
- The first release attempt exposed a Windows-only white-screen deadlock: `WebviewWindowBuilder` was called from a synchronous Tauri command. `open_world_view` is now asynchronous, matching Tauri's documented Windows requirement.
- There was no local Run fixture during the live pass, so desk-to-Run focusing was not exercised against user data. The Rust validation and frontend interaction tests cover that path without creating or mutating a real Run.

## DEV-307 local study protocol

For each recruited participant, randomize two equivalent task sets:

- 2D-only: identify which agent is active, blocked, waiting, and ready for review; open its Run detail.
- 2D+3D: perform the same questions with the world window available but optional.

Record task accuracy, time to first correct answer, wrong-Run clicks, whether the world was opened without prompting, and a one-sentence preference. Do not add telemetry or transmit results automatically; use consented local research records. M3's product gate remains open until this study shows that the world improves understanding or voluntary use without degrading the 2D path.

## Known non-claims

- The world is not an execution surface, simulation game, autonomous agent environment, or second scheduler.
- Capability isolation reduces the frontend IPC surface; it is not an OS sandbox for Rust code or the upstream CLIs.
- Automated tests establish implementation consistency, not human comprehension or cross-device performance.
