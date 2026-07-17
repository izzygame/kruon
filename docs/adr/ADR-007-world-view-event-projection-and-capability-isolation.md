# ADR-007: World View event projection and capability isolation

- Status: Accepted; amended for M3.1 local visual assets
- Date: 2026-07-16
- Scope: M3 minimal world view

## Context

Kruon needs an optional spatial view for quickly understanding the state of Codex and Claude runs. The view must not become another control plane, another source of truth, or a new route to local process, filesystem, approval, or credential operations. Closing or crashing it must leave the M1/M2 2D path and managed runs unaffected.

Tauri custom commands are available to every webview unless they are declared in the application manifest and granted through window-specific capabilities. A visual-only window therefore requires an explicit command manifest, not just the absence of filesystem plugins.

## Decision

1. `EventStore` and M2 review records remain the only inputs to a Rust `WorldSnapshot` projection. The world keeps no durable state and receives no workspace path, prompt, event payload, PID, credential, approval parameter, or artifact content.
2. The fixed mapping is: `idle`, `planning`, `running`, `waiting_approval`, `blocked`, `reviewing`, `completed`, and `sleeping`. Every station includes the source Run ID and latest source event sequence when a Run exists.
3. The world is a separate dynamically created Tauri webview labeled `world`. The async creation command avoids the documented WebView2 deadlock on Windows. React selects the world entry from the current webview label, and React Three Fiber plus Three.js are loaded only there; closing the window destroys the webview and its WebGL renderer.
4. `build.rs` declares every custom command. The `main` capability receives the existing control commands plus `open_world_view`. The `world` capability receives only:
   - `get_world_snapshot`: read the redacted projection;
   - `focus_main_run`: validate an existing Run, emit its ID to `main`, and focus the 2D window.
5. The world capability receives no process, shell, filesystem, dialog, clipboard, network plugin, credential, approval, cancellation, artifact, task, or workspace command.
6. The scene uses a frozen two-desk low-poly layout. M3.1 may replace the procedural furniture and agents with an allowlisted, hashed set of packaged CC0 assets. The first allowlist contains eight Kenney GLB files and one shared 512 px texture totaling 606,458 bytes and approximately 3,581 visible imported triangles. No runtime asset may be fetched from a remote origin.
7. Clicking a desk never changes runtime state. It only focuses the same Run detail in the authoritative 2D window.
8. The imported layer is lazy-loaded inside the already-lazy world renderer. Loading or rendering failure falls back to the original procedural scene. Character animation data may be sampled to a fixed pose, but M3.1 does not enable a continuous animation loop. The renderer keeps demand frames, a DPR cap of 1.25, the low-power GPU preference, 2-second visible polling, and 15-second hidden polling.

## Consequences

- A compromised world frontend has a much smaller IPC surface than the main control window.
- 2D and 3D cannot diverge through independent persistence because both consume the same replayed Run/Event projection.
- The world cannot approve, cancel, execute, write, or access credentials even if its renderer is compromised.
- State freshness is bounded by polling rather than push delivery. This is acceptable for M3 and keeps the world independent from the Run execution path.
- The repository retains the upstream CC0 license files plus a source, size, triangle, and SHA-256 manifest for every selected runtime asset.
- Allowing `'self'` in `connect-src` permits GLB loading from the packaged application origin; it does not permit remote asset or API origins.
- DEV-307 user-comprehension evidence still requires recruited participants; automated implementation evidence cannot substitute for that external study.

## Revisit triggers

- More than two simultaneous agents are displayed.
- A non-CC0 asset, a runtime remote asset, user-authored layout, or a continuous animation loop is proposed.
- The packaged asset payload exceeds 5 MiB, imported visible geometry exceeds 50k triangles, or a texture exceeds 512 px per axis.
- A world interaction is proposed that mutates a Task or Run.
- Measured idle CPU or memory exceeds the M3 budget.
