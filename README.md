# kruon

A local-first desktop workspace for AI coding agents.

## Product positioning

Kruon is a local-first control plane for people who want to organize their own
AI tools without surrendering control of their projects, data, or workflow.

- **No Kruon account required.** Kruon does not require users to sign in to a
  Kruon-operated service to create a workspace, run the local control plane, or
  access local history.
- **Your tools, your choice.** Users bring the CLI tools, subscriptions, and
  providers they already use. A cloud-backed tool still uses that provider's
  own authentication and network connection.
- **Local data by default.** Workspaces, run history, and product state stay on
  the device unless a user explicitly chooses a tool or workflow that sends
  data to an external provider.
- **A verified offline path is planned.** Local-model adapters will make it
  possible to run an end-to-end local workflow. Kruon will only label a run as
  offline after its model, provider endpoint, and supporting workflow have been
  verified as local.
- **Credentials stay local.** Provider credentials belong in OS-backed secure
  storage, never in project files, event history, diagnostic bundles, or a
  silently inherited child-process environment.

Kruon is being prepared for an open-source release. The source is public, but
an OSI-approved license has not yet been selected; the project must not claim a
licensed open-source release until that decision and a `LICENSE` file exist.

## Prerequisites

- **Node.js** >= 20
- **pnpm** >= 9
- **Rust stable** (for the Tauri desktop and W1 runtime core)

## Getting started

```bash
pnpm install
pnpm dev          # Frontend-only Vite server on http://localhost:1420
pnpm desktop:dev  # Tauri desktop app (requires Rust/Cargo)
pnpm typecheck    # TypeScript type checking
pnpm test         # Vitest
pnpm build        # TypeScript check + Vite production build
pnpm desktop:build # Tauri executable without native installer bundle
pnpm check-env    # Environment dependency check
```

## Project structure

```
kruon/
├── apps/
│   └── desktop/        # Vite + React + TypeScript frontend
│       └── src-tauri/  # Tauri 2 Rust runtime, SQLite events and CLI adapters
├── docs/
│   ├── adr/            # Architecture decisions
│   ├── research/       # User research kits
│   └── verification/   # W1 acceptance evidence
├── scripts/
│   └── check-dev-env.mjs
├── .github/workflows/  # CI (Node gate + Tauri gate)
├── package.json
└── pnpm-workspace.yaml
```

## Current boundaries

- M1/M2 provide the local 2D task, run, artifact, audit, and human-review path
  over controlled read-only adapters; Kruon does not claim
  network-egress, container, or Seatbelt isolation.
- W1 has no local-model adapter or credential-settings UI yet. Existing cloud
  CLI integrations may still require each upstream provider's login and network
  access.
- Provider keys are deliberately redacted and excluded from child-process
  inheritance. See [ADR-006](docs/adr/ADR-006-local-first-credential-and-local-model-policy.md)
  for the planned credential and local-model policy.
- Approval fingerprints and audit records exist, but the frozen Codex/Claude
  product adapters currently expose `sandbox_policy_only`; Kruon does not
  present that as verified per-action approval.
- M3 adds an optional, separately permissioned world window. It receives only a
  redacted Run-state projection and can focus an existing Run back in 2D; it
  cannot execute, approve, cancel, read files, write files, or access credentials.
- M3.1 adds a small allowlisted set of packaged Kenney CC0 furniture and character
  assets. They load only inside the world window; no model is fetched remotely,
  and a procedural scene remains the renderer fallback.
- M4 DEV-401 adds a fail-closed Alpha compatibility gate. New Runs currently
  allow Codex `0.144.1`/`0.144.2` and Claude Code `2.1.205`/`2.1.211`; other,
  unparseable, and prerelease versions remain visible but cannot launch until a
  reviewed fixture and matrix update are added.
- M4 DEV-402 bounds each process output stream, tolerates invalid UTF-8 without
  dropping later events, keeps crash/timeout outcomes fail-closed, and makes all
  four local schema migrations transactional. Disk-full and rollback tests use
  disposable databases and never fill the user's real disk.
- M4 DEV-403 exports a bounded, metadata-only JSON diagnostic bundle from the
  main 2D window. It excludes prompts, project/workspace identity, full paths,
  credentials, event payloads, and raw logs; a second privacy scan must pass
  before the file is written, and Kruon never uploads it automatically.
- M4 DEV-404 now has an Apple Silicon app/DMG package contract, Hardened Runtime,
  an empty reviewed entitlement set, a manual private signing/notarization
  workflow, stable retained app-data paths, and a fail-closed newer-schema gate.
  It is not yet a signed/notarized or automatically updating macOS release: the
  Apple credentials, clean-machine lifecycle run, updater key and HTTPS channel
  remain external release gates.
- M4 DEV-405 derives a five-step first-connection path from durable local
  records, adds one idempotent read-only sample task per trusted workspace, and
  turns normalized connection states plus public error codes into concrete
  recovery actions. A compatible but unauthenticated CLI remains launch-blocked;
  login stays in the upstream terminal and Kruon never auto-accepts the sample.
- M4 DEV-406 internal preflight adds revocable Workspace trust, SQLite
  no-follow/private-mode storage hardening, and a mutation-tested CI security
  contract for Tauri capabilities, CSP, read-only adapters, environment and
  storage. Independent review is still open; unrestricted CLI network egress,
  OS-level sandboxing and executable provenance remain explicit high risks.
- M4 DEV-407 repository tooling adds an on-device aggregate readiness view and
  a bounded JSON export that requires fresh consent every time. It contains no
  participant/install identifier, workspace or task text, file locations,
  prompts, logs/events, or automatic upload. The 20–50 person cohort, support
  evidence, and Go/Pivot/No-Go decision remain external and open.
- Native icon files are technical placeholders pending approved brand export.
- Actual 3–5 person user interviews depend on product-side recruitment and consent.

See [W1 acceptance](docs/verification/W1-acceptance-report-2026-07-14.md),
[M2 verification](docs/verification/M2-task-control-loop-2026-07-16.md),
[M3 verification](docs/verification/M3-minimal-world-view-2026-07-16.md),
[M3.1 asset verification](docs/verification/M3.1-kenney-assets-2026-07-16.md),
[M4 compatibility verification](docs/verification/M4-alpha-compatibility-2026-07-16.md),
[M4 fault-injection verification](docs/verification/M4-fault-injection-2026-07-17.md),
[M4 diagnostic verification](docs/verification/M4-diagnostic-export-2026-07-17.md),
[M4 macOS release preflight](docs/verification/M4-macos-alpha-release-preflight-2026-07-17.md),
[M4 onboarding verification](docs/verification/M4-onboarding-recovery-2026-07-17.md),
[M4 security preflight](docs/verification/M4-security-preflight-2026-07-17.md),
[M4 consented metrics verification](docs/verification/M4-alpha-metrics-2026-07-18.md), and
[security review](docs/security/w1-runtime-security-review-2026-07-14.md).

## Verification

| Command            | Expected result                      |
|--------------------|--------------------------------------|
| `pnpm typecheck`   | No type errors                       |
| `pnpm test`        | All tests pass                       |
| `pnpm security:check` | Frozen Alpha capability/CSP/read-only/storage contract passes |
| `pnpm build`       | Produces `apps/desktop/dist/`        |
| `pnpm check-env`   | Node/pnpm/Tauri CLI/Rust/Cargo ✓    |
| `cargo test --all-targets` (in `apps/desktop/src-tauri`) | Rust core and probes pass |
