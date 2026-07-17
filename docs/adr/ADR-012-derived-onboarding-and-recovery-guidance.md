# ADR-012: Derived onboarding and code-driven recovery guidance

- Status: Accepted
- Date: 2026-07-17
- Scope: M4 DEV-405

## Context

Kruon already persists workspaces, tasks, queue entries, runs, events, artifacts, and human reviews. Adding a second onboarding-state store would allow a checkbox to claim completion while the underlying record was missing, failed, or removed. The first-run path also needs to exercise the real control loop without giving a new user a write-capable task or collecting upstream credentials.

Public Rust errors already expose stable codes with redacted messages. A recovery UI should use those codes instead of parsing secret-bearing process output or inventing success after a failed operation.

## Decision

1. Onboarding progress is derived from durable product records: a supported and authenticated connection, an explicitly trusted workspace, the fixed sample task, its queue/run record, and its latest human review. Kruon stores no separate “onboarding complete” flag.
2. The built-in sample is a normal Task with the exact marker `Kruon Alpha onboarding sample. This task is read-only and must not modify files.` and `allowed_paths = ["."]`. It asks only for workspace structure and development entry points and explicitly forbids file changes.
3. Sample creation requires a trusted workspace. A process-local mutex prevents concurrent duplicate creation, and an exact workspace/title/context lookup returns the existing task after restart. No schema migration is required.
4. The sample uses the normal durable queue, fixed read-only adapter plan, Run/Event projection, cancellation, artifact collection, completion report, and human accept/return path. It has no privileged shortcut and is never auto-accepted.
5. A connection is launch-ready only when its CLI version is in the exact Alpha matrix and the upstream CLI reports authenticated. Login remains an upstream terminal action; Kruon does not collect or store the credential.
6. Global recovery guidance is selected from the public error-code prefix. Connection-card recovery is selected from the normalized discovery state. Guidance may recommend refresh, a version/auth command, trust/scope repair, storage repair, or a fresh retry, but it must not expose raw stderr or claim that a failed Run succeeded.
7. Metadata-only diagnostic export remains available as a support action only when the backend is reachable. Its existing privacy boundary is unchanged.

## Consequences

- Restarting Kruon reconstructs the first-connection state from the same source of truth used by the task board.
- The onboarding path becomes a small end-to-end product proof rather than a dismissible welcome screen.
- Authentication and unsupported-version failures block launch earlier and provide a concrete terminal command or supported-version list.
- Rust and TypeScript duplicate the fixed sample marker. Regression tests must keep both exact values aligned until a future typed metadata command removes that duplication.
- Completing repository tests proves the flow wiring, not that a new external user understands it. First-user observation and invite metrics remain DEV-407 evidence.

## Evidence

- `RuntimeCore::create_sample_task` enforces trust, exact lookup, and a concurrency lock.
- The main Tauri capability allowlists only the new sample command; the world window receives no new permission.
- `App.tsx` derives five progress steps and maps public error/connection states to recovery copy.
- Rust tests cover trust and idempotence. Frontend tests cover sample creation, normal queueing, code-driven recovery, and unauthenticated launch blocking.

## Revisit triggers

- changing the sample task from read-only or expanding its allowed paths;
- adding editable/dismissible onboarding state or syncing completion across devices;
- adding local-model adapters that do not use upstream account authentication;
- changing public error-code stability or exposing richer structured recovery payloads;
- enabling automatic acceptance or any onboarding-only execution privilege.
