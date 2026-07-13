# kruon

A local-first desktop workspace for AI coding agents.

## Prerequisites

- **Node.js** >= 20
- **pnpm** >= 9
- **Rust** (for Tauri desktop build — see [Known Blocks](#known-blocks))

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
│       └── src-tauri/  # Tauri 2 Rust shell (blocked locally)
├── scripts/
│   └── check-dev-env.mjs
├── .github/workflows/  # CI (Node gate + Tauri gate)
├── package.json
└── pnpm-workspace.yaml
```

## Known blocks

- **Rust / Cargo** are not installed on the current development machine.
  Tauri 2 `src-tauri/` configuration is structurally correct but cannot be built locally.
  CI's `tauri-checks` job will run after Rust is installed in the runner.
- Native bundling is intentionally disabled until approved kruon application icons are
  generated from the brand source and added to `src-tauri/icons/`.
- The desktop frontend (Vite + React) can be fully developed, type-checked, tested,
  and built without Rust.

## Verification

| Command            | Expected result                      |
|--------------------|--------------------------------------|
| `pnpm typecheck`   | No type errors                       |
| `pnpm test`        | All tests pass                       |
| `pnpm build`       | Produces `apps/desktop/dist/`        |
| `pnpm check-env`   | Node/pnpm/Tauri CLI ✓, Rust/Cargo ✗ |
