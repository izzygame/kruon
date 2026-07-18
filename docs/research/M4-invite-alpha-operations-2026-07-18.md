# M4 invited Alpha operations

- Date: 2026-07-18
- Scope: DEV-407, 20–50 invited target users
- Repository status: tooling and operating contract ready; recruitment and observed evidence not started in this repository

## Cohort and ownership

The target cohort is individual creators and small-team developers who already use Codex, Claude Code, or both in local project work. The Alpha owner recruits 20–50 people, maintains consent and contact details in an access-controlled research system outside this repository, and assigns each participant an operator-side study code. Kruon does not receive or export that code.

Do not commit names, email addresses, chat handles, consent records, support transcripts, exported metric files, project screenshots, or participant-to-file mappings to this repository.

## Invite and consent script

Use the following points before installation:

1. Kruon is an invite-only technical Alpha that launches supported third-party CLIs in read-only mode inside a user-selected trusted workspace.
2. The CLI may still use its own network service. Kruon does not provide complete OS-level filesystem or network isolation.
3. Participation is voluntary and can stop at any time. Declining research or metrics sharing does not block local product use.
4. Kruon stores product state locally. It does not create a Kruon account or automatically upload metrics, diagnostics, prompts, source code, file locations, or logs.
5. The participant may separately consent to an observed session/interview and, after inspecting it, may separately export and share an aggregate Alpha metrics file. These are distinct choices.
6. Never ask a participant to send credentials, prompt text, source files, raw terminal output, or a whole Kruon database.

Record consent externally with: study code, information version, installation consent, observed-session consent, metrics-file sharing consent, recording consent if applicable, timestamp, and withdrawal status. Use the existing `w1-user-interviews` consent and synthesis templates for qualitative sessions.

## Session path

Each first-use session should attempt the same observable path:

1. Install and open Kruon.
2. Reach one compatible, authenticated CLI connection.
3. Trust one disposable or otherwise appropriate workspace.
4. Create and queue the read-only onboarding sample.
5. Inspect the Run result and explicitly accept or return the task.
6. Open Alpha readiness, inspect the local values, choose whether to consent to one export, and share the file only through the approved research channel.

The observer records where help was required, the public error code shown by Kruon, recovery result, and the participant's explanation of what Kruon can and cannot access. Do not copy task content or project details into the study ledger.

## Support severity and response

| Severity | Definition | Alpha response |
|---|---|---|
| P0 | suspected credential/source disclosure, unauthorized write, execution outside the trusted workspace, or uncontrolled process | stop the cohort, preserve only consented minimal evidence, notify the security owner immediately, and do not resume until disposition and regression evidence exist |
| P1 | repeatable data loss/corruption, cancellation failure, privacy-export leak, or install/launch failure affecting multiple participants | pause affected invitations, acknowledge within one working day, provide a safe workaround or rollback, and require a verified fix before expanding the cohort |
| P2 | one participant cannot complete the core first-run/review path but data and security remain intact | triage within two working days, capture public error code and environment class only, and track recovery outcome |
| P3 | cosmetic, copy, discoverability, or optional world-view issue with a working core path | log for weekly review; do not interrupt the cohort unless the issue becomes systematic |

Support must request the metadata-only diagnostic bundle only when the aggregate metrics file is insufficient for diagnosis. Both files remain manual, inspectable exports and are never uploaded by Kruon.

## Metric interpretation

The JSON file is one device snapshot, not an event stream. Aggregate cohort reporting must state both numerator and denominator and must de-duplicate participants in the external ledger, never by adding an identifier to Kruon.

| Measure | Numerator | Denominator | Interpretation guardrail |
|---|---|---|---|
| invite activation | participants who installed and opened | consented invites sent | operator record; not present in the Kruon file |
| first-run completion | participants reaching a terminal completed Run | participants who attempted the sample | combine observed session record with aggregate snapshot; do not count an active Run as failure |
| terminal success | completed terminal Runs | all terminal Runs | file field uses basis points; active Runs excluded |
| review coverage | unique reviewed tasks | all recorded tasks | file field uses basis points; repeated reviews do not increase the numerator |
| time to first Run | earliest Run minus earliest workspace | participants with a valid interval | device-history approximation, not instrumented interaction time |
| support burden | support cases and assisted sessions | activated participants | report by P0–P3 and assisted/unassisted outcome |

Do not average percentages without their denominators. Report median and range for time values, alongside the valid-sample count. Treat a missing value as unavailable, not zero.

## Weekly evidence table

Keep the real participant ledger outside the repository. Only a fully de-identified aggregate may be copied into a release decision document.

| Week | Invited | Activated | Sample attempted | Sample completed | Reviewed | Support P0/P1/P2/P3 | Valid metric files | Decision |
|---|---:|---:|---:|---:|---:|---|---:|---|
| Not started | 0 | 0 | 0 | 0 | 0 | 0/0/0/0 | 0 | External recruitment required |

## Stop and decision rules

Stop invitations immediately for any P0. Pause cohort expansion for an unresolved P1, a new high/critical security finding, an unsupported release artifact, or evidence that the consent/export description is misleading. A repository test failure also blocks a new build but is not itself participant evidence.

The Alpha owner may issue a Go/Pivot/No-Go recommendation only after documenting cohort size, denominators, qualitative findings, support severity, withdrawals, known sampling limitations, DEV-404 release evidence, and DEV-406 independent-review disposition. Until then DEV-407 and M4 remain open.
