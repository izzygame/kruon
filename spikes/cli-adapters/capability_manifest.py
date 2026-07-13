"""Capability manifests and frozen launch plans.

Holds the curated per-adapter capability snapshots (loaded from
``snapshots/*.json``), the set of dangerous bypass flags kruon must never
pass, and the :func:`build_launch_plan` helper that freezes a run's
arguments + approval mode and fingerprints them.

Per the plan (section 6), the ``LaunchPlan`` is frozen and fingerprinted
*before* execution; any parameter change requires re-policy + re-approval.
Per the plan (section 3.4), Codex ``exec`` has no per-action approval flag,
so its approval mode is honestly declared ``sandbox_policy_only``; Claude
Code with ``--permission-mode manual`` + stream-json targets ``per_action``
-- but that target is ``inferred`` until a real run-loop spike confirms the
permission_request round-trip (see README R-01).
"""

from __future__ import annotations

import hashlib
import json
import os
from dataclasses import asdict, dataclass, field
from typing import Any, Dict, List, Optional

# --------------------------------------------------------------------------- #
# Constants
# --------------------------------------------------------------------------- #

CAPABILITY_SCHEMA_VERSION = "1.0"
APPROVAL_MODES = ("per_action", "sandbox_policy_only", "none")

# Evidence levels for capability declarations.
EVIDENCE_LEVELS = ("verified", "inferred", "unverified", "unsupported")

# Flags that bypass approval/sandbox/permission checks. kruon NEVER passes
# these; build_launch_plan raises if any appear in the arg list. Verified
# present in help text (evidence/*.txt) for both CLIs.
DANGEROUS_FLAGS = (
    "--dangerously-bypass-approvals-and-sandbox",  # codex
    "--dangerously-bypass-hook-trust",  # codex
    "--dangerously-skip-permissions",  # claude
    "--allow-dangerously-skip-permissions",  # claude
)

# Codex sandbox modes (from `codex exec --help`). workspace-write is the
# safest mode that still lets the agent edit the trusted workspace.
CODEX_SANDBOX_MODES = ("read-only", "workspace-write", "danger-full-access")

# Claude permission modes (from `claude --help`). Only `manual` targets
# per-action approval; `bypassPermissions` is forbidden.
CLAUDE_PERMISSION_MODES = (
    "acceptEdits",
    "auto",
    "bypassPermissions",  # forbidden in kruon
    "manual",
    "dontAsk",
    "plan",
)

FORBIDDEN_PERMISSION_MODES = ("bypassPermissions",)


class UnsafeLaunchPlanError(ValueError):
    """Raised when a launch plan would pass a dangerous flag or forbidden mode."""


# --------------------------------------------------------------------------- #
# Dataclasses
# --------------------------------------------------------------------------- #


@dataclass
class CapabilitySnapshot:
    """Curated capability manifest for one adapter version.

    The ``evidence`` field on each capability distinguishes ``verified``
    (demonstrated by a real run / this spike's --version/--help capture) from
    ``inferred`` (documented but not yet run-loop-proven).
    """

    adapter: str
    tool_name: str
    version: Optional[str]
    approval_mode: str
    schema_version: str = CAPABILITY_SCHEMA_VERSION
    interfaces: List[Dict[str, Any]] = field(default_factory=list)
    capabilities: Dict[str, Dict[str, Any]] = field(default_factory=dict)
    notes: List[str] = field(default_factory=list)

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)


@dataclass
class LaunchPlan:
    """Frozen, fingerprinted description of an adapter run.

    Built before execution. The fingerprint covers the tool, task, workspace,
    approval mode and the exact argv -- so any parameter drift invalidates a
    prior approval.
    """

    adapter: str
    tool_name: str
    workspace: str
    task: str
    approval_mode: str
    argv: List[str]
    stdin_payload: str
    env_redacted: Dict[str, str] = field(default_factory=dict)
    fingerprint: str = ""

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)


# --------------------------------------------------------------------------- #
# Approval-mode determination
# --------------------------------------------------------------------------- #


def determine_approval_mode(
    adapter: str, *, claude_permission_mode: Optional[str] = None
) -> str:
    """Static approval-mode determination from launch flags.

    This is a *capability declaration* based on documented flags, not a claim
    about a specific run's observed behavior.

    * codex exec: ``sandbox_policy_only`` (no --ask-for-approval in exec mode;
      verified absent from `codex exec --help`).
    * claude -p --permission-mode manual: ``per_action`` (target; inferred --
        the permission_request stream-json round-trip is not help-text-proven).
    * claude -p with acceptEdits/auto/dontAsk/plan: ``none`` (no gating) or
        ``sandbox_policy_only`` for plan (policy-limited). We map auto/dontAsk
        to ``none`` and plan/acceptEdits to ``sandbox_policy_only``.
    """
    if adapter == "codex":
        return "sandbox_policy_only"
    if adapter == "claude":
        mode = claude_permission_mode or "manual"
        if mode in FORBIDDEN_PERMISSION_MODES:
            raise UnsafeLaunchPlanError(
                f"permission mode {mode!r} is forbidden in kruon (bypasses all checks)"
            )
        if mode == "manual":
            return "per_action"
        if mode in ("plan", "acceptEdits"):
            return "sandbox_policy_only"
        # auto / dontAsk / unknown -> no gating.
        return "none"
    raise ValueError(f"unknown adapter: {adapter!r}")


# --------------------------------------------------------------------------- #
# Launch plan construction
# --------------------------------------------------------------------------- #


def _check_safe_args(argv: List[str]) -> None:
    for flag in DANGEROUS_FLAGS:
        for arg in argv:
            if arg == flag or arg.startswith(flag + "="):
                raise UnsafeLaunchPlanError(
                    f"refusing to build launch plan with dangerous flag {flag!r}"
                )


def _fingerprint(plan_args: Dict[str, Any]) -> str:
    blob = json.dumps(plan_args, sort_keys=True, ensure_ascii=False, separators=(",", ":"))
    return hashlib.sha256(blob.encode("utf-8")).hexdigest()[:16]


def build_launch_plan(
    adapter: str,
    *,
    task: str,
    workspace: str,
    claude_permission_mode: Optional[str] = None,
    codex_sandbox: str = "workspace-write",
    extra_args: Optional[List[str]] = None,
    model: Optional[str] = None,
    max_budget_usd: Optional[float] = None,
) -> LaunchPlan:
    """Build a frozen, fingerprinted LaunchPlan for an adapter run.

    Refuses to include any dangerous bypass flag. Does NOT execute anything.
    The returned argv is what a supervisor *would* spawn (only with explicit
    model-call opt-in + execution in probe.py).
    """
    if adapter not in ("codex", "claude"):
        raise ValueError(f"unknown adapter: {adapter!r}")
    if not task.strip():
        raise ValueError("task must be non-empty")
    workspace = os.path.abspath(workspace)
    if not os.path.isdir(workspace):
        # Allow non-existent for tests, but normalize.
        pass
    if codex_sandbox not in CODEX_SANDBOX_MODES:
        raise ValueError(f"invalid codex_sandbox: {codex_sandbox!r}")

    approval_mode = determine_approval_mode(adapter, claude_permission_mode=claude_permission_mode)
    extra_args = list(extra_args or [])

    argv: List[str] = []
    tool_name = adapter
    if adapter == "codex":
        # Non-interactive, JSONL events, frozen sandbox policy. No
        # --ask-for-approval (unavailable in exec mode). No dangerous bypass.
        argv = [
            "codex",
            "exec",
            "--json",
            "-s",
            codex_sandbox,
            "--skip-git-repo-check",
        ]
        if model:
            argv += ["-c", f'model="{model}"']
        argv += extra_args
        argv += ["-"]
    else:  # claude
        mode = claude_permission_mode or "manual"
        if mode in FORBIDDEN_PERMISSION_MODES:
            raise UnsafeLaunchPlanError(f"permission mode {mode!r} is forbidden")
        argv = [
            "claude",
            "-p",
            "--output-format",
            "stream-json",
            "--input-format",
            "stream-json",
            "--permission-mode",
            mode,
        ]
        if model:
            argv += ["--model", model]
        if max_budget_usd is not None:
            argv += ["--max-budget-usd", str(max_budget_usd)]
        argv += extra_args

    _check_safe_args(argv)
    _check_safe_args(extra_args)

    stdin_payload = task
    if adapter == "claude":
        # Claude stream-json input is intentionally kept off the process list.
        # The exact bidirectional message shape remains an inferred spike item.
        stdin_payload = json.dumps(
            {"type": "user", "message": {"role": "user", "content": task}},
            ensure_ascii=False,
        ) + "\n"

    plan = LaunchPlan(
        adapter=adapter,
        tool_name=tool_name,
        workspace=workspace,
        task=task,
        approval_mode=approval_mode,
        argv=argv,
        stdin_payload=stdin_payload,
        env_redacted={},
    )
    plan.fingerprint = _fingerprint(
        {
            "adapter": adapter,
            "workspace": workspace,
            "task": task,
            "approval_mode": approval_mode,
            "argv": argv,
            "stdin_payload": stdin_payload,
        }
    )
    return plan


# --------------------------------------------------------------------------- #
# Snapshot load / save
# --------------------------------------------------------------------------- #


def load_snapshot(path: str) -> CapabilitySnapshot:
    with open(path, "r", encoding="utf-8") as fh:
        data = json.load(fh)
    return CapabilitySnapshot(
        schema_version=data.get("schema_version", CAPABILITY_SCHEMA_VERSION),
        adapter=data["adapter"],
        tool_name=data["tool_name"],
        version=data.get("version"),
        approval_mode=data["approval_mode"],
        interfaces=data.get("interfaces", []),
        capabilities=data.get("capabilities", {}),
        notes=data.get("notes", []),
    )


def save_snapshot(snapshot: CapabilitySnapshot, path: str) -> None:
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(snapshot.to_dict(), fh, ensure_ascii=False, indent=2)
        fh.write("\n")


# --------------------------------------------------------------------------- #
# Capability manifest schema validation
# --------------------------------------------------------------------------- #


def _load_capability_schema() -> Dict[str, Any]:
    schema_path = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                               "capability_manifest.schema.json")
    with open(schema_path, "r", encoding="utf-8") as fh:
        return json.load(fh)


def validate_capability_manifest(
    manifest: Dict[str, Any],
) -> List[str]:
    """Validate a capability manifest dict against the schema.

    Returns a list of error strings (empty = valid).
    Uses the spike's own mini_jsonschema, no third-party dependency.
    """
    from mini_jsonschema import validate as schema_validate  # noqa: E402
    schema = _load_capability_schema()
    return schema_validate(manifest, schema)


def validate_snapshot_file(path: str) -> List[str]:
    """Load a snapshot JSON file and validate it against the schema."""
    with open(path, "r", encoding="utf-8") as fh:
        data = json.load(fh)
    return validate_capability_manifest(data)
