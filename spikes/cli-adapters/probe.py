#!/usr/bin/env python3
"""kruon dual-CLI capability probe (W1 spike S1-04 / S1-05).

USAGE
-----
    # Default (SAFE): only --version / --help for both CLIs. No model calls.
    python3 probe.py

    # Probe one tool only.
    python3 probe.py --tool codex

    # Inspect a frozen launch plan WITHOUT spawning anything (dry-run).
    python3 probe.py --allow-model-call --task "list files" --workspace "$PWD"

    # Actually spawn a model call (TWO explicit keys required). Never used by
    # default; costs tokens and needs upstream auth.
    python3 probe.py --allow-model-call --execute \
        --tool claude --task "say hello" --workspace "$PWD"

    # Self-test the fixture suite against the parser + schema.
    python3 probe.py --fixtures-self-test

SAFETY MODEL
------------
* By default the probe runs ONLY ``--version`` and ``--help`` for each CLI.
  These are side-effect-free and emit no credentials. Output is redacted
  before it is stored or printed.
* Real model calls require TWO explicit opt-ins: ``--allow-model-call`` AND
  ``--execute``. With only ``--allow-model-call`` the probe builds and prints
  a frozen :class:`LaunchPlan` (with fingerprint) but does NOT spawn.
* Dangerous bypass flags (``--dangerously-bypass-approvals-and-sandbox``,
  ``--dangerously-skip-permissions``, ...) are NEVER emitted; the launch-plan
  builder raises if any appear.
* The probe never reads, prints or stores upstream tokens. Redaction scrubs
  obvious credential shapes from captured stdout/stderr.

This is a SPIKE: help-text observations are facts; run-loop behavior (real
permission_request round-trips, codex app-server per-action channels) is
inference until a real model-call spike confirms it. See README.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from dataclasses import asdict, dataclass, field
from typing import Any, Dict, List, Optional, Tuple

# Allow running as a script and as a module.
HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

import capability_manifest as cm
import event_parser as ep
from mini_jsonschema import validate as schema_validate

# --------------------------------------------------------------------------- #
# Command execution
# --------------------------------------------------------------------------- #


@dataclass
class CmdResult:
    argv: List[str]
    exit_code: Optional[int]
    stdout: str  # redacted
    stderr: str  # redacted
    timed_out: bool = False
    error: Optional[str] = None  # e.g. "not_installed"

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)


def run_cmd(argv: List[str], timeout: float = 15.0) -> CmdResult:
    """Run ``argv`` capturing output. Never raises; failures -> CmdResult.error."""
    try:
        proc = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
    except FileNotFoundError:
        return CmdResult(argv=argv, exit_code=None, stdout="", stderr="", error="not_installed")
    except subprocess.TimeoutExpired as exc:
        out = ep.redact((exc.stdout or "") if isinstance(exc.stdout, str) else "")
        err = ep.redact((exc.stderr or "") if isinstance(exc.stderr, str) else "")
        return CmdResult(
            argv=argv, exit_code=None, stdout=out, stderr=err, timed_out=True, error="timeout"
        )
    except Exception as exc:  # pragma: no cover - defensive
        return CmdResult(argv=argv, exit_code=None, stdout="", stderr="", error=f"exec_error: {exc}")

    return CmdResult(
        argv=argv,
        exit_code=proc.returncode,
        stdout=ep.redact(proc.stdout or ""),
        stderr=ep.redact(proc.stderr or ""),
    )


# --------------------------------------------------------------------------- #
# Version + help-text analysis (FACTS, not run-loop proof)
# --------------------------------------------------------------------------- #

_VERSION_RE = re.compile(r"(\d+(?:\.\d+)*(?:[-.\w]*))")


def parse_version(text: str, adapter: str) -> Optional[str]:
    """Best-effort version extraction from --version output."""
    text = (text or "").strip().splitlines()[0] if (text or "").strip() else ""
    if adapter == "codex":
        # "codex-cli 0.144.0-alpha.4"
        m = _VERSION_RE.search(text)
        return m.group(1) if m else None
    if adapter == "claude":
        # "2.1.205 (Claude Code)"
        m = re.match(r"\s*(\d+(?:\.\d+)*(?:[-.\w]*))", text)
        return m.group(1) if m else None
    m = _VERSION_RE.search(text)
    return m.group(1) if m else None


def analyze_help_for_approval(adapter: str, help_text: str) -> Dict[str, Any]:
    """Static, fact-only analysis of help text for approval-related flags.

    Returns flags that are DOCUMENTED in help text. This is explicitly NOT
    evidence of run-loop behavior -- only that the flag string appears.
    """
    if adapter == "codex":
        return {
            "has_ask_for_approval": "--ask-for-approval" in help_text,
            "has_sandbox": "--sandbox" in help_text,
            "has_json": "--json" in help_text,
            "dangerous_flags_documented": [
                f for f in cm.DANGEROUS_FLAGS if f in help_text
            ],
            "note": (
                "exec --help has NO --ask-for-approval (verified by help text). "
                "Per-action approval is therefore not declared for codex exec."
            ),
        }
    if adapter == "claude":
        return {
            "has_permission_mode": "--permission-mode" in help_text,
            "has_stream_json": "stream-json" in help_text,
            "has_include_hook_events": "--include-hook-events" in help_text,
            "has_max_budget": "--max-budget-usd" in help_text,
            "permission_mode_choices_documented": _extract_permission_choices(help_text),
            "dangerous_flags_documented": [
                f for f in cm.DANGEROUS_FLAGS if f in help_text
            ],
            "note": (
                "--permission-mode manual + bidirectional stream-json are documented; "
                "the permission_request stream-json message shape is INFERRED, not "
                "help-text-proven (see README R-01)."
            ),
        }
    return {}


def _extract_permission_choices(help_text: str) -> List[str]:
    block = re.search(
        r"--permission-mode(?P<body>.*?)(?=\n\s+--[a-zA-Z]|\Z)", help_text, re.DOTALL
    )
    if not block:
        return []
    return re.findall(r'"([a-zA-Z]+)"', block.group("body"))


# --------------------------------------------------------------------------- #
# Per-tool probe
# --------------------------------------------------------------------------- #


def probe_tool(adapter: str, timeout: float = 15.0) -> Dict[str, Any]:
    """Run --version and --help for one adapter. Returns an observed snapshot."""
    binary = adapter  # codex / claude
    version_res = run_cmd([binary, "--version"], timeout=timeout)

    help_cmds: List[List[str]] = [[binary, "--help"]]
    if adapter == "codex":
        help_cmds.append([binary, "exec", "--help"])
    elif adapter == "claude":
        help_cmds.append([binary, "-p", "--help"])

    help_results = []
    for cmd in help_cmds:
        r = run_cmd(cmd, timeout=timeout)
        help_results.append(
            {
                "argv": r.argv,
                "exit_code": r.exit_code,
                "stdout_excerpt": ep.truncate(r.stdout, 600),
                "error": r.error,
            }
        )

    version = None
    installed = version_res.error != "not_installed"
    if version_res.exit_code == 0:
        version = parse_version(version_res.stdout, adapter)

    # Analyze only the most specific command help. Mixing top-level Codex help
    # with `codex exec --help` would falsely report interactive-only flags as
    # exec capabilities.
    specific_help = run_cmd(help_cmds[-1], timeout=timeout)
    full_help = (
        specific_help.stdout
        if specific_help.error is None and specific_help.exit_code == 0
        else ""
    )

    approval_analysis = analyze_help_for_approval(adapter, full_help) if full_help else {}

    return {
        "adapter": adapter,
        "installed": installed,
        "binary": binary,
        "version": version,
        "version_exit_code": version_res.exit_code,
        "approval_help_analysis": approval_analysis,
        "help_commands": help_results,
        "approval_mode_declared": cm.determine_approval_mode(adapter),
        "evidence_level": "help_text_only",  # NOT run-loop proof
    }


# --------------------------------------------------------------------------- #
# Fixture self-test
# --------------------------------------------------------------------------- #


def _load_schema() -> Dict[str, Any]:
    with open(os.path.join(HERE, "normalized_event.schema.json"), "r", encoding="utf-8") as fh:
        return json.load(fh)


def fixtures_self_test(verbose: bool = False) -> Tuple[bool, List[Dict[str, Any]]]:
    """Parse every fixture and validate normalized events against the schema.

    Returns (all_passed, per_fixture_results).
    """
    schema = _load_schema()
    with open(os.path.join(HERE, "fixtures", "manifest.json"), "r", encoding="utf-8") as fh:
        manifest = json.load(fh)

    results: List[Dict[str, Any]] = []
    all_passed = True

    for fx in manifest["fixtures"]:
        path = os.path.join(HERE, "fixtures", fx["path"])
        with open(path, "r", encoding="utf-8") as fh:
            text = fh.read()
        approval_mode = fx["expected"].get("approval_mode")
        events, errors = ep.parse_stream(text, fx["adapter"], approval_mode=approval_mode)
        terminal, exit_code = ep.classify_terminal(events)
        degraded_count = sum(1 for e in events if e.degraded)

        # Schema-validate each normalized event.
        schema_errors: List[str] = []
        for e in events:
            schema_errors += schema_validate(e.to_dict(), schema)

        exp = fx["expected"]
        checks = {
            "terminal_state": terminal == exp["terminal_state"],
            "parse_errors": len(errors) == exp.get("parse_errors", 0),
            "degraded_count_ok": degraded_count == exp.get("degraded_count", 0)
            if "degraded_count" in exp
            else True,
            "events_emitted": len(events) >= exp.get("events_min", 1),
            "schema_valid": len(schema_errors) == 0,
        }
        if "exit_code" in exp and exp["exit_code"] is not None:
            checks["exit_code"] = exit_code == exp["exit_code"]

        passed = all(checks.values())
        all_passed = all_passed and passed
        results.append(
            {
                "id": fx["id"],
                "adapter": fx["adapter"],
                "passed": passed,
                "checks": checks,
                "observed_terminal": terminal,
                "observed_exit_code": exit_code,
                "parse_errors": len(errors),
                "degraded_count": degraded_count,
                "schema_errors": schema_errors[:3],
            }
        )
        if verbose:
            print(f"  [{('PASS' if passed else 'FAIL')}] {fx['id']}: {checks}")

    return all_passed, results


# --------------------------------------------------------------------------- #
# Model-call execution (TWO-key opt-in; never default)
# --------------------------------------------------------------------------- #


def execute_model_call(plan: cm.LaunchPlan, timeout: float = 120.0) -> Dict[str, Any]:
    """Spawn the CLI per ``plan`` and stream-parse its output.

    TWO-key opt-in enforced by main(): ``--allow-model-call --execute``.

    On the first ``approval.request`` event, reading stops and the process is
    terminated -- kruon never auto-approves. The bidirectional approval
    response loop is future work (M2).
    """
    if not os.path.isdir(plan.workspace):
        return {"error": "workspace_not_found", "workspace": plan.workspace}

    try:
        proc = subprocess.Popen(
            plan.argv,
            cwd=plan.workspace,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
    except FileNotFoundError:
        return {"error": "not_installed", "adapter": plan.adapter}

    events: List[ep.NormalizedEvent] = []
    parse_errors: List[ep.ParseError] = []
    blocked_on_approval = False
    assert proc.stdin is not None
    proc.stdin.write(plan.stdin_payload)
    proc.stdin.close()
    assert proc.stdout is not None
    try:
        for line in proc.stdout:
            ev, err = (
                ep.parse_codex_line(line, len(events) + len(parse_errors) + 1, plan.approval_mode)
                if plan.adapter == "codex"
                else ep.parse_claude_obj(
                    json.loads(line) if line.strip() else {},
                    len(events) + len(parse_errors) + 1,
                    plan.approval_mode,
                )
            )
            if ev:
                events.append(ev)
                if ev.event_type == "approval.request":
                    blocked_on_approval = True
                    break
            if err:
                parse_errors.append(err)
    finally:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()

    terminal, exit_code = ep.classify_terminal(events)
    return {
        "adapter": plan.adapter,
        "approval_mode": plan.approval_mode,
        "fingerprint": plan.fingerprint,
        "events": len(events),
        "parse_errors": len(parse_errors),
        "terminal_state": terminal,
        "exit_code": exit_code,
        "blocked_on_approval": blocked_on_approval,
        "evidence_level": "run_loop_observed",
    }


# --------------------------------------------------------------------------- #
# CLI
# --------------------------------------------------------------------------- #


def build_arg_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="probe.py",
        description="kruon dual-CLI capability probe (default: --version/--help only).",
    )
    p.add_argument("--tool", choices=["codex", "claude", "both"], default="both")
    p.add_argument("--timeout", type=float, default=15.0)
    p.add_argument("--snapshot-dir", default=os.path.join(HERE, "snapshots"))
    p.add_argument("--write-observed", action="store_true",
                   help="write observed probe snapshot JSON to <snapshot-dir>/observed/")
    # Model-call opt-in (disabled by default; two-key).
    p.add_argument("--allow-model-call", action="store_true",
                   help="opt-in gate for real model calls (does NOT spawn without --execute)")
    p.add_argument("--execute", action="store_true",
                   help="actually spawn the CLI (requires --allow-model-call)")
    p.add_argument("--task", help="task prompt (required for model-call path)")
    p.add_argument("--workspace", help="trusted workspace dir (required for model-call path)")
    p.add_argument("--claude-permission-mode", default="manual")
    p.add_argument("--codex-sandbox", default="workspace-write")
    p.add_argument("--model", default=None)
    p.add_argument("--max-budget-usd", type=float, default=None)
    # Self-tests.
    p.add_argument("--fixtures-self-test", action="store_true")
    p.add_argument("--verbose", "-v", action="store_true")
    return p


def main(argv: Optional[List[str]] = None) -> int:
    args = build_arg_parser().parse_args(argv)

    if args.fixtures_self_test:
        print("Running fixture self-test (parser + schema)...")
        ok, results = fixtures_self_test(verbose=args.verbose)
        if args.verbose:
            print(json.dumps(results, ensure_ascii=False, indent=2))
        print(f"{'PASS' if ok else 'FAIL'}: {sum(1 for r in results if r['passed'])}/{len(results)} fixtures")
        return 0 if ok else 1

    # Model-call path (two-key opt-in).
    if args.allow_model_call:
        if not args.task or not args.workspace:
            print("ERROR: --allow-model-call requires --task and --workspace", file=sys.stderr)
            return 2
        adapter = args.tool if args.tool in ("codex", "claude") else "claude"
        try:
            plan = cm.build_launch_plan(
                adapter,
                task=args.task,
                workspace=args.workspace,
                claude_permission_mode=args.claude_permission_mode,
                codex_sandbox=args.codex_sandbox,
                model=args.model,
                max_budget_usd=args.max_budget_usd,
            )
        except cm.UnsafeLaunchPlanError as exc:
            print(f"ERROR: unsafe launch plan: {exc}", file=sys.stderr)
            return 2
        print("Frozen LaunchPlan (no spawn):")
        print(json.dumps(plan.to_dict(), ensure_ascii=False, indent=2))
        if not args.execute:
            print("\n(dry-run; pass --execute to spawn. TWO-key opt-in required.)")
            return 0
        print("\n--execute set: spawning model call...")
        result = execute_model_call(plan, timeout=args.timeout)
        print(json.dumps(result, ensure_ascii=False, indent=2))
        return 0

    # Default safe path: --version / --help only.
    tools = ["codex", "claude"] if args.tool == "both" else [args.tool]
    summary: Dict[str, Any] = {"evidence_level": "help_text_only", "tools": {}}
    for tool in tools:
        snap = probe_tool(tool, timeout=args.timeout)
        summary["tools"][tool] = snap
        print(f"=== {tool} ===")
        print(f"  installed : {snap['installed']}")
        print(f"  version   : {snap['version']}")
        print(f"  approval  : {snap['approval_mode_declared']} (declared)")
        print(f"  help analysis: {json.dumps(snap['approval_help_analysis'], ensure_ascii=False)}")
        print()

    if args.write_observed:
        obs_dir = os.path.join(args.snapshot_dir, "observed")
        os.makedirs(obs_dir, exist_ok=True)
        for tool in tools:
            with open(os.path.join(obs_dir, f"{tool}_probe.json"), "w", encoding="utf-8") as fh:
                json.dump(summary["tools"][tool], fh, ensure_ascii=False, indent=2)
                fh.write("\n")
        print(f"(observed snapshots written to {obs_dir})")

    print("NOTE: evidence is help-text only; not run-loop proof. See README.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
