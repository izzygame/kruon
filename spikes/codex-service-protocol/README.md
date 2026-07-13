# Codex Service Protocol Spike (W1 S1-04)

## Objective

Determine the correct service protocol strategy for Kruon's Codex integration layer. Evaluate whether Kruon should use `per_action` approval, JSON-RPC protocol (app-server), or `sandbox_policy_only` for its agentic service.

## Scope

- Codex CLI version: **0.144.2** (evidence saved in `evidence/codex-version.txt`)
- Layers examined: `exec`, `app-server`, `exec-server`
- Artifacts generated: JSON Schema bundle, TypeScript bindings, capability analyzer + tests

## Key Findings

### 1. Command Existence (read from evidence/*.txt)

| Layer | Status | Key subcommands |
|---|---|---|
| `codex exec` | Present | `resume`, `review`, `--output-schema`, `--json`, `--ephemeral` |
| `codex app-server` | Present (experimental) | `daemon`, `proxy`, `generate-ts`, `generate-json-schema` |
| `codex exec-server` | Present (experimental) | `--listen ws://`, `--remote`, `--environment-id`, `--use-agent-identity-auth` |

### 2. Protocol Schema Surface

- **V1 schema**: 82 definitions, covering InitializeParams/Response + approval types
- **V2 schema**: 516 definitions, covering command exec, FS ops, config, auth, apps, feedback, experimental features
- **TS bindings**: ~90 files (v1) + ~510 files (v2)

### 3. Approval Mechanisms (protocol_declare confirmed)

| Approval Type | Protocol Evidence |
|---|---|
| `ExecCommandApproval` | Schema declares ExecCommandApprovalParams/Response |
| `CommandExecutionRequestApproval` | Schema declares CommandExecutionRequestApprovalParams/Response |
| `FileChangeRequestApproval` | Schema declares FileChangeRequestApprovalParams/Response |
| `ApplyPatchApproval` | Schema declares ApplyPatchApprovalParams/Response |
| `PermissionsRequestApproval` | Schema declares PermissionsRequestApprovalParams/Response |
| `ToolRequestUserInput` | Schema declares ToolRequestUserInputParams/Response (EXPERIMENTAL) |

### 4. Session Lifecycle

- `resume` (interactive + exec), `archive`, `unarchive`, `delete`, `fork`
- App-server declares `CommandExecTerminateParams` for a `command/exec` process; whole-turn/run cancellation remains unverified
- No single `Artifact` abstraction exists; file-change notifications are lower-level artifact candidates

### 5. Sandbox Policies (command_exists confirmed)

- `read-only`, `workspace-write`, `danger-full-access`
- `codex exec --help` does not expose a per-action approval flag

## Evidence Levels

- **command_exists**: CLI flag/subcommand present in `--help` (raw text saved in `evidence/*.txt`)
- **protocol_declare**: JSON Schema or TS type declares the message
- **closed_loop**: End-to-end verified (not done -- requires live model inference)

## Recommendation

See [decision.md](decision.md).

## Files

| File | Description |
|---|---|
| `evidence/` | Raw root/exec/app-server/exec-server `--help` and `--version` output (re-collectable evidence) |
| `schema/` | JSON Schema bundles (v1 + v2) |
| `ts-bindings/` | TypeScript type definitions |
| `capability_analyzer.py` | Pure stdlib capability analyzer (reads evidence/*.txt) |
| `test_capability_analyzer.py` | Unittests for analyzer |
| `capability_report.txt` | Generated analysis report |
| `decision.md` | Protocol strategy decision |
