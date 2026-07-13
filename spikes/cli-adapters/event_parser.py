"""Standardized adapter event parser.

Converts raw Codex ``exec --json`` JSONL and Claude Code ``stream-json`` lines
into a single :class:`NormalizedEvent` shape (see
``normalized_event.schema.json``).

Design notes
------------
* Raw event shapes below are *representative* of the documented upstream
  formats, hand-authored for this spike. They are NOT live captures: real
  model calls are opt-in and disabled by default (see ``probe.py``). Field
  names that could not be confirmed from help text alone (notably Claude's
  ``permission_request`` stream-json message) are tagged ``# INFERRED`` and
  flagged in README "Facts vs Inferences". The parser is resilient: an
  unknown ``type`` becomes a ``degraded`` event with the raw payload
  preserved, never a crash and never a guessed terminal state.
* ``approval_mode`` is stamped onto every event by the caller (it is a
  property of the *run*, derived from the frozen launch plan), not inferred
  from a single raw event. This keeps the per-action vs sandbox-policy
  asymmetry honest.
* No randomness / no wall-clock: event ids are deterministic
  (``adapter:line_no:raw_type``) so replay is stable.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
from dataclasses import asdict, dataclass, field
from typing import Any, Dict, Iterator, List, Optional, Tuple

SCHEMA_VERSION = "1.0"

# Approval modes mirror normalized_event.schema.json exactly.
APPROVAL_MODES = ("per_action", "sandbox_policy_only", "none")

TERMINAL_STATES = ("completed", "failed", "cancelled", "forced_stop_required", "unknown")


# --------------------------------------------------------------------------- #
# Secret redaction
# --------------------------------------------------------------------------- #

# Patterns are deliberately conservative: only obvious credential shapes.
# Path redaction for diagnostics is a separate concern (probe.diagnostics).
_SECRET_PATTERNS: List[Tuple[str, re.Pattern]] = [
    ("openai_key", re.compile(r"sk-[A-Za-z0-9_-]{20,}")),
    ("bearer_token", re.compile(r"(?i)Bearer\s+[A-Za-z0-9._\-]{12,}")),
    ("github_token", re.compile(r"gh[pousr]_[A-Za-z0-9]{20,}")),
    ("google_api_key", re.compile(r"AIza[0-9A-Za-z_\-]{20,}")),
    ("slack_token", re.compile(r"xox[baprs]-[A-Za-z0-9-]+")),
    ("anthropic_key", re.compile(r"sk-ant-[A-Za-z0-9_\-]{20,}")),
]

# KEY=VALUE where the key name looks like a secret. Catches `export API_KEY=...`.
_SECRET_ENV_PATTERN = re.compile(
    r"(?m)(?P<prefix>^|\s)(?P<key>(?=[A-Z0-9_]*(?:SECRET|TOKEN|PASSWORD|API_KEY|APIKEY|ACCESS_KEY|CREDENTIAL))[A-Z][A-Z0-9_]*)\s*=\s*(?P<val>[^\s]+)"
)


def redact(text: str) -> str:
    """Return ``text`` with obvious credential shapes replaced.

    Non-string input is stringified first. Never raises.
    """
    if not isinstance(text, str):
        text = str(text)
    for kind, pat in _SECRET_PATTERNS:
        text = pat.sub(f"[REDACTED:{kind}]", text)
    text = _SECRET_ENV_PATTERN.sub(
        lambda m: f"{m.group('prefix')}{m.group('key')}=[REDACTED:env_secret]", text
    )
    return text


def _redact_obj(obj: Any) -> Any:
    """Recursively redact strings inside nested dict/list structures."""
    if isinstance(obj, str):
        return redact(obj)
    if isinstance(obj, dict):
        return {k: _redact_obj(v) for k, v in obj.items()}
    if isinstance(obj, list):
        return [_redact_obj(v) for v in obj]
    return obj


def truncate(s: str, n: int = 2000) -> str:
    if len(s) <= n:
        return s
    return s[: n - 3] + "..."


def fingerprint_params(params: Any) -> str:
    """Stable sha256 fingerprint of bound approval parameters (canonical JSON).

    Parameter drift (any change in command/args/paths) changes this hash, which
    invalidates a prior approval per the plan's parameter-bound approval model.
    """
    blob = json.dumps(params, sort_keys=True, ensure_ascii=False, separators=(",", ":"))
    return hashlib.sha256(blob.encode("utf-8")).hexdigest()[:16]


# --------------------------------------------------------------------------- #
# Dataclasses
# --------------------------------------------------------------------------- #


@dataclass
class NormalizedEvent:
    schema_version: str = SCHEMA_VERSION
    adapter: str = ""
    event_id: str = ""
    session_id: Optional[str] = None
    ts: Any = None
    event_type: str = ""
    phase: str = "running"
    approval_mode: Optional[str] = None
    approval_request: Optional[Dict[str, Any]] = None
    content: Optional[Dict[str, Any]] = None
    artifact: Optional[Dict[str, Any]] = None
    terminal_state: Optional[str] = None
    exit_code: Optional[int] = None
    degraded: bool = False
    source: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        d = asdict(self)
        # Drop optional fields that are None so strict-typed schema properties
        # (terminal_state enum; content/artifact/approval_request objects) are
        # not violated by null. Scalar nulls the schema explicitly allows
        # (session_id, ts, exit_code) are kept for diagnostics.
        for key in ("terminal_state", "approval_request", "artifact", "content"):
            if d.get(key) is None:
                d.pop(key, None)
        return d


@dataclass
class ParseError:
    line_no: int
    message: str
    raw: str  # already redacted + truncated

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)


# --------------------------------------------------------------------------- #
# Helpers
# --------------------------------------------------------------------------- #


def _event_id(adapter: str, line_no: int, raw_type: str) -> str:
    return f"{adapter}:{line_no}:{raw_type or 'unknown'}"


def _path_in_workspace(path: str, workspace: Optional[str]) -> bool:
    """True if ``path`` is relative or resolves inside ``workspace``.

    Used to flag out-of-workspace artifacts as policy candidates. With no
    workspace given, relative paths are trusted and absolute paths are not.
    """
    if not path:
        return False
    if not os.path.isabs(path):
        return True
    if workspace is None:
        return False
    try:
        return os.path.realpath(path).startswith(os.path.realpath(workspace))
    except Exception:
        return False


def _extract_text(content: Any) -> Optional[str]:
    """Pull a flat text string from a content array (Claude/codex style)."""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = []
        for item in content:
            if isinstance(item, dict):
                if item.get("type") == "text" and isinstance(item.get("text"), str):
                    parts.append(item["text"])
                elif isinstance(item.get("text"), str):
                    parts.append(item["text"])
            elif isinstance(item, str):
                parts.append(item)
        return "\n".join(parts) if parts else None
    return None


# --------------------------------------------------------------------------- #
# Codex exec --json JSONL parser
# --------------------------------------------------------------------------- #
#
# Representative raw shape (per codex `exec --json` docs / project findings):
#   {"type": "session.created", "payload": {"session_id": "...", "cwd": "..."}}
#   {"type": "message", "payload": {"role": "assistant", "content": [...]}}
#   {"type": "command_executed", "payload": {"command": [...], "cwd": "...", "exit_code": 0}}
#   {"type": "file_update", "payload": {"path": "...", "change": "..."}}
#   {"type": "turn.completed", "payload": {"status": "completed", "last_agent_message": "..."}}
#   {"type": "error", "payload": {"message": "..."}}
#
# NOTE: codex `exec --help` has NO `--ask-for-approval` flag (verified via help
# text in evidence/). Per-action approval is therefore NOT available in exec
# mode; approval_mode for codex exec runs is `sandbox_policy_only` and tool
# calls are NOT emitted as approval requests.


def parse_codex_line(
    line: str,
    line_no: int,
    approval_mode: str = "sandbox_policy_only",
    workspace: Optional[str] = None,
) -> Tuple[Optional[NormalizedEvent], Optional[ParseError]]:
    line = line.rstrip("\n")
    if not line.strip():
        return None, None
    try:
        obj = json.loads(line)
    except json.JSONDecodeError as exc:
        return None, ParseError(
            line_no=line_no,
            message=f"malformed JSON: {exc.msg}",
            raw=truncate(redact(line)),
        )
    if not isinstance(obj, dict):
        return None, ParseError(
            line_no=line_no,
            message="root event is not a JSON object",
            raw=truncate(redact(line)),
        )

    raw_type = str(obj.get("type", "unknown"))
    payload = obj.get("payload") if isinstance(obj.get("payload"), dict) else {}
    payload = _redact_obj(payload)
    eid = _event_id("codex", line_no, raw_type)

    common = dict(
        adapter="codex",
        event_id=eid,
        session_id=payload.get("session_id"),
        ts=obj.get("ts") or payload.get("ts"),
        approval_mode=approval_mode,
        source={
            "raw_type": raw_type,
            "raw_line": line_no,
            "raw_json": truncate(redact(json.dumps(obj, ensure_ascii=False))),
        },
    )

    if raw_type == "session.created":
        return NormalizedEvent(
            event_type="session.start", phase="setup", content={"cwd": payload.get("cwd")}, **common
        ), None

    if raw_type == "message":
        role = payload.get("role", "unknown")
        text = _extract_text(payload.get("content"))
        etype = "assistant.message" if role == "assistant" else f"{role}.message"
        return NormalizedEvent(
            event_type=etype, phase="running", content={"role": role, "text": text}, **common
        ), None

    if raw_type in ("command_executed", "shell_command_call", "exec_command_begin"):
        cmd = payload.get("command", payload.get("args"))
        exit_code = payload.get("exit_code")
        # In sandbox_policy_only mode tool calls are NOT approval requests.
        phase = "tool_call"
        return NormalizedEvent(
            event_type="tool.call",
            phase=phase,
            content={"command": cmd, "exit_code": exit_code},
            exit_code=exit_code if isinstance(exit_code, int) else None,
            **common,
        ), None

    if raw_type in ("file_update", "file_update_begin", "patch_apply"):
        path = str(payload.get("path", ""))
        return NormalizedEvent(
            event_type="artifact.file",
            phase="artifact",
            artifact={
                "path": path,
                "kind": payload.get("change", "modify"),
                "in_workspace": _path_in_workspace(path, workspace),
            },
            **common,
        ), None

    if raw_type in ("turn.completed", "task.complete", "turn.complete"):
        status = str(payload.get("status", "")).lower()
        if status in ("completed", "success", "complete"):
            tstate, ecode = "completed", 0
        elif status in ("failed", "error", "failure"):
            tstate, ecode = "failed", payload.get("exit_code", 1)
        elif status in ("cancelled", "canceled", "aborted"):
            tstate, ecode = "cancelled", None
        else:
            tstate, ecode = "unknown", None
        return NormalizedEvent(
            event_type="task.complete",
            phase="terminal",
            terminal_state=tstate,
            exit_code=ecode if isinstance(ecode, int) else None,
            content={"last_agent_message": payload.get("last_agent_message")},
            **common,
        ), None

    if raw_type == "error":
        # Treat top-level error as a degraded signal; classify_terminal will
        # promote to failed if no explicit terminal event follows.
        return NormalizedEvent(
            event_type="error",
            phase="degraded",
            degraded=True,
            content={"message": payload.get("message")},
            **common,
        ), None

    # Unknown codex event type -> degraded, raw preserved, no guessed state.
    return NormalizedEvent(
        event_type="unknown",
        phase="degraded",
        degraded=True,
        content={"payload": payload},
        **common,
    ), None


# --------------------------------------------------------------------------- #
# Claude Code stream-json parser
# --------------------------------------------------------------------------- #
#
# Representative raw shape (per Claude Code `--output-format stream-json` docs):
#   {"type":"system","subtype":"init","session_id":"...","cwd":"..."}
#   {"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"..."}]}}
#   {"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{...}}]}}
#   {"type":"user","message":{"content":[{"type":"tool_result","content":"..."}]}}
#   {"type":"result","subtype":"success","result":"...","is_error":false,"total_cost_usd":0.01}
#   {"type":"permission_request","permission":{"tool_name":"Bash","input":{...}}}   # INFERRED
#
# The `permission_request` message shape is INFERRED: `--permission-mode manual`
# and bidirectional `stream-json` are documented in help text, but the exact
# request/response message fields are NOT confirmed without a real run-loop
# spike. See README "Facts vs Inferences" and open risk R-01.


def parse_claude_obj(
    obj: Dict[str, Any],
    line_no: int,
    approval_mode: str = "per_action",
    workspace: Optional[str] = None,
) -> Tuple[Optional[NormalizedEvent], Optional[ParseError]]:
    obj = _redact_obj(obj)
    raw_type = str(obj.get("type", "unknown"))
    eid = _event_id("claude", line_no, raw_type)

    common = dict(
        adapter="claude",
        event_id=eid,
        session_id=obj.get("session_id"),
        ts=obj.get("ts"),
        approval_mode=approval_mode,
        source={
            "raw_type": raw_type,
            "raw_line": line_no,
            "raw_json": truncate(redact(json.dumps(obj, ensure_ascii=False))),
        },
    )

    if raw_type == "system":
        subtype = obj.get("subtype", "")
        if subtype == "init":
            return NormalizedEvent(
                event_type="session.start",
                phase="setup",
                content={"cwd": obj.get("cwd"), "model": obj.get("model")},
                **common,
            ), None
        return NormalizedEvent(
            event_type=f"system.{subtype}" if subtype else "system",
            phase="running",
            **common,
        ), None

    if raw_type == "assistant":
        message = obj.get("message", {}) if isinstance(obj.get("message"), dict) else {}
        content = message.get("content", [])
        text = _extract_text(content)
        tool_uses = [c for c in content if isinstance(c, dict) and c.get("type") == "tool_use"]
        if tool_uses:
            tu = tool_uses[0]
            return NormalizedEvent(
                event_type="tool.call",
                phase="tool_call",
                content={"tool": tu.get("name"), "input": tu.get("input")},
                **common,
            ), None
        return NormalizedEvent(
            event_type="assistant.message",
            phase="running",
            content={"text": text},
            **common,
        ), None

    if raw_type == "user":
        message = obj.get("message", {}) if isinstance(obj.get("message"), dict) else {}
        content = message.get("content", [])
        results = [c for c in content if isinstance(c, dict) and c.get("type") == "tool_result"]
        if results:
            return NormalizedEvent(
                event_type="tool.result",
                phase="running",
                content={"content": results[0].get("content")},
                **common,
            ), None
        return NormalizedEvent(
            event_type="user.message", phase="running", content={"text": _extract_text(content)}, **common
        ), None

    if raw_type == "permission_request":  # INFERRED shape
        perm = obj.get("permission", {}) if isinstance(obj.get("permission"), dict) else {}
        tool_name = perm.get("tool_name", "unknown")
        params = perm.get("input", {})
        kind = {"Bash": "shell_command", "Write": "file_write", "Edit": "file_write"}.get(
            tool_name, tool_name.lower()
        )
        return NormalizedEvent(
            event_type="approval.request",
            phase="waiting_approval",
            approval_request={
                "kind": kind,
                "fingerprint": fingerprint_params({"tool": tool_name, "input": params}),
                "params": params,
                "expires_at": obj.get("expires_at"),
            },
            content={"tool": tool_name},
            **common,
        ), None

    if raw_type == "result":
        subtype = str(obj.get("subtype", "")).lower()
        is_error = bool(obj.get("is_error"))
        if subtype == "success" and not is_error:
            tstate, ecode = "completed", 0
        elif subtype == "error" or is_error:
            tstate, ecode = "failed", 1
        elif subtype in ("cancelled", "canceled"):
            tstate, ecode = "cancelled", None
        else:
            tstate, ecode = "unknown", None
        return NormalizedEvent(
            event_type="task.complete",
            phase="terminal",
            terminal_state=tstate,
            exit_code=ecode,
            content={
                "result": obj.get("result"),
                "total_cost_usd": obj.get("total_cost_usd"),
                "is_error": is_error,
            },
            **common,
        ), None

    # Unknown claude event type -> degraded, raw preserved.
    return NormalizedEvent(
        event_type="unknown",
        phase="degraded",
        degraded=True,
        content={"raw": obj},
        **common,
    ), None


# --------------------------------------------------------------------------- #
# Stream iterators
# --------------------------------------------------------------------------- #


def iter_codex_jsonl(
    text: str,
    approval_mode: str = "sandbox_policy_only",
    workspace: Optional[str] = None,
) -> Iterator[Tuple[Optional[NormalizedEvent], Optional[ParseError]]]:
    for i, line in enumerate(text.splitlines(), start=1):
        yield parse_codex_line(line, i, approval_mode=approval_mode, workspace=workspace)


def iter_claude_stream_json(
    text: str,
    approval_mode: str = "per_action",
    workspace: Optional[str] = None,
) -> Iterator[Tuple[Optional[NormalizedEvent], Optional[ParseError]]]:
    for i, line in enumerate(text.splitlines(), start=1):
        if not line.strip():
            yield None, None
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError as exc:
            yield None, ParseError(
                line_no=i, message=f"malformed JSON: {exc.msg}", raw=truncate(redact(line))
            )
            continue
        if not isinstance(obj, dict):
            yield None, ParseError(
                line_no=i, message="root event is not a JSON object", raw=truncate(redact(line))
            )
            continue
        yield parse_claude_obj(obj, i, approval_mode=approval_mode, workspace=workspace)


def parse_stream(
    text: str,
    adapter: str,
    approval_mode: str = "sandbox_policy_only",
    workspace: Optional[str] = None,
) -> Tuple[List[NormalizedEvent], List[ParseError]]:
    """Parse a full raw stream into (events, errors)."""
    events: List[NormalizedEvent] = []
    errors: List[ParseError] = []
    it = iter_codex_jsonl(text, approval_mode, workspace) if adapter == "codex" else (
        iter_claude_stream_json(text, approval_mode, workspace)
    )
    for ev, err in it:
        if ev is not None:
            events.append(ev)
        if err is not None:
            errors.append(err)
    return events, errors


# --------------------------------------------------------------------------- #
# Terminal reconciliation
# --------------------------------------------------------------------------- #


def classify_terminal(
    events: List[NormalizedEvent],
) -> Tuple[str, Optional[int]]:
    """Reconcile a parsed event list into (terminal_state, exit_code).

    Rules:
    * An explicit terminal event with a known state wins.
    * If terminal events conflict (e.g. completed then failed), the run is
      ``unknown`` (uncertain) -- never silently coerced to completed.
    * No terminal event but a degraded ``error`` present -> ``failed``.
    * No terminal event at all -> ``unknown`` (uncertain). This is the honest
      outcome for an abruptly-cancelled stream; the supervisor may later
      upgrade it to ``forced_stop_required`` (see resolve_cancel_state).
    """
    terminals = [e for e in events if e.phase == "terminal" and e.terminal_state]
    if terminals:
        states = {e.terminal_state for e in terminals}
        if states == {"completed"}:
            return "completed", terminals[-1].exit_code
        if states == {"failed"}:
            return "failed", terminals[-1].exit_code
        if states == {"cancelled"}:
            return "cancelled", terminals[-1].exit_code
        if "unknown" in states and len(states) == 1:
            return "unknown", terminals[-1].exit_code
        # Mixed/conflicting terminal signals -> uncertain.
        return "unknown", terminals[-1].exit_code

    if any(e.event_type == "error" and e.degraded for e in events):
        err = next(e for e in events if e.event_type == "error" and e.degraded)
        return "failed", err.exit_code

    return "unknown", None


def resolve_cancel_state(
    observed_terminal: str,
    cancel_requested: bool,
    responded_within_deadline: bool,
) -> str:
    """Supervisor-side resolution of an observed terminal state after a cancel.

    Maps the *observed* CLI state to a kruon *resolved* state:
    * If the CLI already reported a clean terminal (completed/failed/cancelled),
      trust it.
    * If a cancel was requested but the CLI went silent (observed ``unknown``)
      and did not respond within the deadline, escalate to
      ``forced_stop_required`` so the process tree can be forcibly torn down.
    * If a cancel was requested and the CLI responded in time, ``cancelled``.
    """
    if observed_terminal in ("completed", "failed"):
        return observed_terminal
    if not cancel_requested:
        return observed_terminal  # unknown stays unknown
    if responded_within_deadline:
        return "cancelled"
    return "forced_stop_required"
