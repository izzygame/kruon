# Decision: Codex Service Protocol Strategy for Kruon

## Options

| Strategy | Description |
|---|---|
| `per_action` | Candidate via app-server JSON-RPC approval request/response messages; the root interactive approval flag alone is not an exec integration channel |
| `JSON-RPC` | Use `codex app-server` or `codex exec-server` as a long-running daemon with JSON-RPC over stdio/Unix socket/WebSocket |
| `sandbox_policy_only` | Use `codex exec --json --sandbox workspace-write`; kruon can apply launch-time policy but has no per-action callback on this surface |

## Evidence Summary

### Command Exists

- `--ask-for-approval` flag: **confirmed on root CLI** (values: `untrusted`, `on-request`, `never`)
- `--ask-for-approval` flag: **NOT present on `codex exec`** subcommand
- `--sandbox` flag: **confirmed** (values: `read-only`, `workspace-write`, `danger-full-access`)
- `codex app-server` daemon mode: **confirmed** (experimental)
- `codex exec-server` standalone service: **confirmed** (experimental)

### Protocol Declare

- Approval protocol types: **confirmed** for exec command, file change, apply_patch, permissions, tool_request_user_input
- Process termination type: **declared** for app-server `command/exec`; whole-turn/run cancellation and process-tree cleanup are not verified
- No single `Artifact` abstraction is declared; file-change notifications provide lower-level artifact candidates

### Closed Loop

- **Not verified**. All three strategies require either model inference (exec) or a running daemon (app-server/exec-server). Neither was executed per spike boundary constraints.

## Recommendation: `sandbox_policy_only` (current implementation baseline)

**Recommended strategy: `sandbox_policy_only` (via `--sandbox workspace-write`)**

Rationale:

1. **`per_action` is not available on `codex exec`**. The `--ask-for-approval` flag only exists on the root (interactive) CLI, not on the `exec` subcommand. Since Kruon's service layer will use `codex exec` for non-interactive task execution, `per_action` is not a viable strategy for the current CLI version.

2. **`JSON-RPC` (app-server/exec-server) is experimental**. The protocol schema is large (516 v2 definitions) and in flux. The approval message types are declared in the schema (protocol_declare confirmed), but the end-to-end bidirectional approval flow has not been verified (closed_loop = false). Relying on it for Kruon's core service protocol introduces unacceptable instability risk at this stage.

3. **`sandbox_policy_only` is the next implementation baseline, not a production-security proof**. The CLI exposes `--sandbox workspace-write` on `codex exec`; no real run, cancellation, recovery, or escape test was performed in this spike.

4. **The bypass flag is NOT recommended**. It is explicitly labeled "EXTREMELY DANGEROUS" and intended solely for externally-sandboxed environments. It must never appear in any recommended Kruon command.

### Implementation Notes

- Use `codex exec --json --sandbox workspace-write` as the initial invocation shape
- Kruon may apply launch-time policy and post-event detection above the process; it must not present this as per-action approval
- Monitor `codex app-server` graduation from experimental for W2+ re-evaluation
- When a controlled live run is authorized, verify app-server initialize, request correlation, approval response, cancellation and recovery as one closed loop

## Evidence Classification

| Evidence | `per_action` | `JSON-RPC` | `sandbox_policy_only` |
|---|---|---|---|
| Command exists | partial (root only, not exec) | confirmed | confirmed |
| Protocol declare | confirmed | confirmed | partial |
| Closed loop | not verified | not verified | not verified |
