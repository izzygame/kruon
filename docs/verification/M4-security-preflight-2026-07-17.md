# M4 security preflight verification

- Date: 2026-07-17
- Scope: DEV-406 internal preflight and external-review preparation
- Decision: ADR-013
- Result: repository preflight implemented; independent external review remains open

## Confirmed fixes

| Finding | Fix | Regression evidence |
|---|---|---|
| Workspace trust could be granted but not revoked | protected `untrust_workspace` command, main-window UI, enqueue and dispatch trust gates | Rust revocation test and frontend command test |
| On-disk SQLite followed default path/permission behavior | shared opener with `SQLITE_OPEN_NOFOLLOW`, immediate symlink rejection, Unix directory `0700`, database `0600` | cross-platform open test; Unix mode/symlink test compiled for target CI |
| Security-sensitive repository boundaries could drift silently | security-contract script in normal CI and signed macOS workflow | 3 mutation tests cover world control, direct start, unsafe script CSP, writable adapter, and followable DB |

## Automated evidence

| Check | Result |
|---|---|
| Rust core | 55 tests passed on Windows |
| W1 probe guards | 2 tests passed |
| Frontend | 3 files / 18 tests passed |
| TypeScript | `pnpm typecheck` passed |
| Security drift gate | 3 Node tests and `pnpm security:check` passed |
| Node production advisory query | official npm registry returned no known vulnerabilities on 2026-07-17 |
| Rust advisory query | not run: `cargo audit` is not installed; required in external review/CI follow-up |
| Release/package regression | macOS package contract 3 tests plus `pnpm release:check` passed; Windows `pnpm desktop:build` produced the updated executable |
| Formatting/diff hygiene | final Rust formatting and `git diff --check` passed; temporary Cargo mirror configuration was removed |

The default `npmmirror.com` registry does not implement npm's advisory bulk endpoint, so the evidence command explicitly used `https://registry.npmjs.org`. No registry configuration was changed.

## Open high-risk boundaries

- unrestricted child-process network egress;
- no OS-level filesystem sandbox and remaining path TOCTOU;
- executable provenance is not established by a version string;
- Unix process-group escape and PID reuse uncertainty;
- real macOS signing/notarization and clean-user lifecycle evidence;
- independent adversarial review of a frozen commit.

These are release decisions, not hidden test failures. The current product must remain read-only, fail-closed, invite-only, and explicit about the absence of complete isolation.
