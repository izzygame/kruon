# kruon

A local-first desktop workspace for AI coding agents.

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

- W1 supports controlled local read-only Codex and Claude runs; it does not claim
  network-egress, container, or Seatbelt isolation.
- The Rust product path does not yet expose per-tool approval fingerprints.
- Native icon files are technical placeholders pending approved brand export.
- Actual 3–5 person user interviews depend on product-side recruitment and consent.

See [W1 acceptance](docs/verification/W1-acceptance-report-2026-07-14.md) and
[security review](docs/security/w1-runtime-security-review-2026-07-14.md).

## Verification

| Command            | Expected result                      |
|--------------------|--------------------------------------|
| `pnpm typecheck`   | No type errors                       |
| `pnpm test`        | All tests pass                       |
| `pnpm build`       | Produces `apps/desktop/dist/`        |
| `pnpm check-env`   | Node/pnpm/Tauri CLI/Rust/Cargo ✓    |
| `cargo test --all-targets` (in `apps/desktop/src-tauri`) | Rust core and probes pass |
