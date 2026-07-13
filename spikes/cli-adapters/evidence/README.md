# evidence/

Raw `--version` / `--help` captures used to ground the help-text FACTS in this
spike. Captured on 2026-07-11 with:

```sh
codex --version          > codex_version.txt
codex --help             > codex_help.txt
codex exec --help        > codex_exec_help.txt
claude --version         > claude_version.txt
claude --help            > claude_help.txt
claude -p --help         > claude_p_help.txt
```

## IMPORTANT — what this evidence is and is NOT

- **IS**: proof that a flag string / subcommand / version number appears in the
  CLI's own `--help` / `--version` output on this machine, on this date.
- **IS NOT**: proof that the flag works as described in a real non-interactive
  run loop, that a documented event message has a particular shape, or that a
  bidirectional approval round-trip actually completes. Per the plan (section
  3.4 / 6), capability must be proven by real fixtures, not help text.

## Files

| file | what | key fact used by the spike |
|---|---|---|
| `codex_version.txt` | `codex --version` | `codex-cli 0.144.0-alpha.4` |
| `claude_version.txt` | `claude --version` | `2.1.205 (Claude Code)` |
| `codex_help.txt` | top-level help | `--ask-for-approval` IS documented (interactive TUI) |
| `codex_exec_help.txt` | `codex exec --help` | `--ask-for-approval` is **ABSENT**; `--json`, `--sandbox`, `resume` present |
| `claude_help.txt` | `claude --help` | `--permission-mode` (with `manual`), `stream-json`, `--include-hook-events`, `--max-budget-usd` documented |
| `claude_p_help.txt` | `claude -p --help` | print/stream-json mode details |

The single most important asymmetry verified here:

```
$ grep -c ask-for-approval evidence/codex_exec_help.txt   # -> 0  (ABSENT)
$ grep -c ask-for-approval evidence/codex_help.txt        # -> 1  (present, TUI only)
```

This is the help-text basis for declaring Codex `exec` as
`sandbox_policy_only` and for the W1 decision gate on `app-server`/`exec-server`.
