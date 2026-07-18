# ADR-014: Consented local Alpha metrics

- Status: Accepted with external evidence gate
- Date: 2026-07-18
- Scope: M4 DEV-407

## Context

DEV-407 needs evidence from 20–50 invited target users, but Kruon is local-first and does not require a Kruon account. Adding a participant identifier, background telemetry, or an automatic upload service only to measure the Alpha would create a new privacy and security boundary before the product has justified it. Repository tests can prove that an evidence tool is safe and usable; they cannot prove recruitment, consent, support quality, or user outcomes.

## Decision

1. Kruon provides a local, aggregate-only Alpha readiness view and JSON export. It does not add telemetry, analytics SDKs, a collection endpoint, or automatic upload.
2. Every export requires a fresh explicit checkbox. Consent is not persisted, and a denied or missing consent writes no file.
3. The export contains current app/platform metadata, adapter launch-readiness booleans, cumulative onboarding-stage booleans, aggregate counts, one device-level timing, and denominator-defined rates only.
4. The export contains no installation, participant, workspace, task, queue, Run, or review identifier; no names, prompt/task text, file locations, event payloads, process output, credentials, or raw records.
5. The existing diagnostic privacy validator scans the serialized metrics bundle before disk write. A forbidden field fails closed. The export is capped at 64 KiB and is written through a create-new temporary file followed by an atomic rename.
6. If the Alpha operator needs to associate a consent record or support case with an export, that mapping stays in an access-controlled research ledger outside Kruon and outside this repository. Kruon does not embed the mapping key.
7. Repository completion means the local measurement/support workflow is ready. DEV-407 and M4 remain open until real participants are recruited, consent is recorded, support cases are handled, aggregate results are reported, and a Go/Pivot/No-Go decision is made.

## Metric semantics

- `launchReadyTool`: at least one Codex or Claude connection is ready, compatible, and authenticated at export time.
- Onboarding funnel stages are device-level cumulative booleans derived from durable local state; they are not remote user events.
- `workspaceToFirstRunSeconds`: elapsed time from the earliest recorded workspace to the earliest recorded Run. It is absent if either timestamp is unavailable or the ordering is invalid.
- `terminalSuccessBasisPoints`: completed terminal Runs divided by all terminal Runs. Active Runs are excluded from the denominator.
- `taskReviewCoverageBasisPoints`: unique tasks with at least one review divided by all recorded tasks.
- A missing rate is serialized as `null`, never coerced to zero.

## Consequences

- An invited user can inspect the on-device result before deciding whether to export or share it.
- Kruon cannot silently build a cross-device or longitudinal participant profile.
- Cohort aggregation and consent custody require a human Alpha operator and a separate controlled ledger.
- Aggregate files alone do not establish usability, comprehension, or causality; observation and interview evidence remain required.

## Evidence

- `core/alpha_metrics.rs` owns aggregation, privacy validation, bounded atomic export, and failure-path tests.
- `RuntimeCore::export_alpha_metrics` collects the existing durable local state without changing it.
- The main-window Tauri command/capability and Alpha readiness panel expose the explicit per-export consent flow.
- `docs/research/M4-invite-alpha-operations-2026-07-18.md` defines recruitment, support, metric interpretation, and stop rules.
- `docs/verification/M4-alpha-metrics-2026-07-18.md` records repository verification and the still-open external evidence gate.

## Revisit triggers

- adding any server collection, telemetry SDK, persistent consent, participant/install identifier, or automatic upload;
- adding event-level, longitudinal, workspace-level, or task-level analytics;
- changing metric denominators or using the metrics for claims beyond the invited Alpha;
- introducing account sync, remote control, crash reporting, or support-session recording.
