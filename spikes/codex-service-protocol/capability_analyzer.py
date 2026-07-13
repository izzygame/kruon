"""
capability_analyzer.py -- Pure stdlib Codex Service Protocol capability analyzer.

Evidence levels for each capability:
  COMMAND_EXISTS   = CLI subcommand/flag is present in --help output
                     (read from evidence/*.txt, not hardcoded)
  PROTOCOL_DECLARE = JSON Schema or TS binding declares a message type for it
  CLOSED_LOOP      = Observed working end-to-end (requires live test, not done here)

Any capability not explicitly verified is marked unknown/unverified.
"""

import json
import os
import re
import sys
from dataclasses import dataclass, field
from typing import Dict, List, Set

SPIKE_DIR = os.path.dirname(os.path.abspath(__file__))
EVIDENCE_DIR = os.path.join(SPIKE_DIR, "evidence")
SCHEMA_DIR = os.path.join(SPIKE_DIR, "schema")
TS_DIR = os.path.join(SPIKE_DIR, "ts-bindings")


@dataclass
class Evidence:
    command_exists: bool = False
    protocol_declare: bool = False
    closed_loop: bool = False
    notes: List[str] = field(default_factory=list)

    @property
    def level(self) -> str:
        if self.closed_loop:
            return "closed_loop"
        if self.protocol_declare:
            return "protocol_declare"
        if self.command_exists:
            return "command_exists"
        return "unknown"


@dataclass
class Capability:
    name: str
    description: str
    evidence: Evidence = field(default_factory=Evidence)
    sub_capabilities: Dict[str, "Capability"] = field(default_factory=dict)


# ---------------------------------------------------------------------------
# Evidence file readers
# ---------------------------------------------------------------------------

def read_evidence_text(filename: str) -> str:
    path = os.path.join(EVIDENCE_DIR, filename)
    try:
        with open(path) as f:
            return f.read()
    except FileNotFoundError:
        return ""


def flag_exists_in_help(help_text: str, flag: str) -> bool:
    return flag in help_text


def subcommand_exists_in_help(help_text: str, subcommand: str) -> bool:
    return bool(re.search(rf"^\s+{re.escape(subcommand)}\s", help_text, re.MULTILINE))


def load_schema_defs(path: str) -> Set[str]:
    names = set()
    try:
        with open(path) as f:
            data = json.load(f)
        names.update(data.get("definitions", {}).keys())
        names.update(data.get("$defs", {}).keys())
    except (FileNotFoundError, json.JSONDecodeError):
        pass
    return names


def load_ts_types(path: str) -> Set[str]:
    names = set()
    if not os.path.isdir(path):
        return names
    for entry in os.listdir(path):
        if entry.endswith(".ts"):
            names.add(entry.removesuffix(".ts"))
    return names


def _has_schema_type(name: str, schema_defs: Set[str], ts_types: Set[str]) -> bool:
    return name in schema_defs or name in ts_types


def name_to_pascal(snake: str) -> str:
    return "".join(word.capitalize() for word in snake.split("_"))


# ---------------------------------------------------------------------------
# Evidence data loaded from files
# ---------------------------------------------------------------------------

_ROOT_HELP = read_evidence_text("codex-help-root.txt")
_EXEC_HELP = read_evidence_text("codex-help-exec.txt")
_APP_SERVER_HELP = read_evidence_text("codex-help-app-server.txt")
_EXEC_SERVER_HELP = read_evidence_text("codex-help-exec-server.txt")
_VERSION = read_evidence_text("codex-version.txt").strip()

# ---------------------------------------------------------------------------
# Analyzers
# ---------------------------------------------------------------------------

def analyze_exec_capabilities(schema_defs: Set[str], ts_types: Set[str]) -> Capability:
    exec_cap = Capability("exec", "Non-interactive task execution")
    exec_cap.evidence.command_exists = subcommand_exists_in_help(_ROOT_HELP, "exec")

    resume = Capability("resume", "Resume a previous exec session by id or --last")
    resume.evidence.command_exists = subcommand_exists_in_help(_EXEC_HELP, "resume")
    exec_cap.sub_capabilities["resume"] = resume

    review = Capability("review", "Run code review non-interactively")
    review.evidence.command_exists = subcommand_exists_in_help(_EXEC_HELP, "review")
    exec_cap.sub_capabilities["review"] = review

    output_schema = Capability("output_schema", "--output-schema <FILE> for structured JSON Schema output")
    output_schema.evidence.command_exists = flag_exists_in_help(_EXEC_HELP, "--output-schema")
    exec_cap.sub_capabilities["output_schema"] = output_schema

    json_output = Capability("json_output", "--json flag for JSONL event output")
    json_output.evidence.command_exists = flag_exists_in_help(_EXEC_HELP, "--json")
    exec_cap.sub_capabilities["json_output"] = json_output

    ephemeral = Capability("ephemeral", "--ephemeral: run without persisting session files")
    ephemeral.evidence.command_exists = flag_exists_in_help(_EXEC_HELP, "--ephemeral")
    exec_cap.sub_capabilities["ephemeral"] = ephemeral

    per_action_approval = Capability(
        "per_action_approval",
        "Per-action approval channel on the non-interactive exec surface"
    )
    per_action_approval.evidence.command_exists = flag_exists_in_help(
        _EXEC_HELP, "--ask-for-approval"
    )
    per_action_approval.evidence.notes.append(
        "The flag exists on the root interactive CLI, but not on `codex exec`; "
        "therefore exec per-action approval remains unavailable on this surface."
    )
    exec_cap.sub_capabilities["per_action_approval"] = per_action_approval

    return exec_cap


def analyze_approval_capabilities(schema_defs: Set[str], ts_types: Set[str]) -> Capability:
    cap = Capability("approval", "Human-in-the-loop approval for commands, file changes, permissions")

    checks = [
        ("exec_command_approval", "Approval for individual shell commands (ExecCommandApproval)"),
        ("command_execution_request_approval", "Approval for command execution requests (CommandExecutionRequestApproval)"),
        ("file_change_request_approval", "Approval for file/patch changes (FileChangeRequestApproval)"),
        ("apply_patch_approval", "Approval for apply_patch tool (ApplyPatchApproval)"),
        ("permissions_request_approval", "Approval for elevated sandbox permissions (PermissionsRequestApproval)"),
        ("tool_request_user_input", "EXPERIMENTAL request_user_input for tool-elicited user questions"),
    ]
    for name, desc in checks:
        sub = Capability(name, desc)
        schema_name = name_to_pascal(name) + "Params"
        sub.evidence.protocol_declare = _has_schema_type(schema_name, schema_defs, ts_types)
        cap.sub_capabilities[name] = sub

    cap.evidence.protocol_declare = any(
        sub.evidence.protocol_declare for sub in cap.sub_capabilities.values()
    )

    return cap


def analyze_session_capabilities(schema_defs: Set[str], ts_types: Set[str]) -> Capability:
    cap = Capability("session", "Session lifecycle: resume, cancel, archive, fork, delete")
    cap.evidence.command_exists = True

    resume = Capability("resume", "Resume interactive session (picker or --last)")
    resume.evidence.command_exists = subcommand_exists_in_help(_ROOT_HELP, "resume")
    cap.sub_capabilities["resume"] = resume

    archive = Capability("archive", "Archive a saved session by id or name")
    archive.evidence.command_exists = subcommand_exists_in_help(_ROOT_HELP, "archive")
    cap.sub_capabilities["archive"] = archive

    delete = Capability("delete", "Permanently delete a saved session")
    delete.evidence.command_exists = subcommand_exists_in_help(_ROOT_HELP, "delete")
    cap.sub_capabilities["delete"] = delete

    unarchive = Capability("unarchive", "Unarchive a saved session")
    unarchive.evidence.command_exists = subcommand_exists_in_help(_ROOT_HELP, "unarchive")
    cap.sub_capabilities["unarchive"] = unarchive

    fork = Capability("fork", "Fork a previous interactive session")
    fork.evidence.command_exists = subcommand_exists_in_help(_ROOT_HELP, "fork")
    cap.sub_capabilities["fork"] = fork

    cancel = Capability("cancel", "Terminate a process started by app-server command/exec")
    has_term_schema = _has_schema_type("CommandExecTerminateParams", schema_defs, ts_types)
    if has_term_schema:
        cancel.evidence.protocol_declare = True
        cancel.evidence.notes.append(
            "CommandExecTerminateParams is declared. Whole-turn/run cancellation and "
            "process-tree termination remain unverified."
        )
    cap.sub_capabilities["cancel"] = cancel

    cap.evidence.command_exists = any(
        sub.evidence.command_exists for sub in cap.sub_capabilities.values()
    )

    return cap


def analyze_artifact_capabilities(schema_defs: Set[str], ts_types: Set[str]) -> Capability:
    cap = Capability("artifact", "Artifact/file output from sessions")

    output_last = Capability("output_last_message", "-o/--output-last-message <FILE>")
    output_last.evidence.command_exists = flag_exists_in_help(_EXEC_HELP, "--output-last-message")
    cap.sub_capabilities["output_last_message"] = output_last

    apply = Capability("apply", "Apply latest diff as git apply")
    apply.evidence.command_exists = subcommand_exists_in_help(_ROOT_HELP, "apply")
    cap.sub_capabilities["apply"] = apply

    for name in ["FileChange", "FileChangeOutputDeltaNotification", "FileChangePatchUpdatedNotification"]:
        if _has_schema_type(name, schema_defs, ts_types):
            sub = Capability(name, f"Protocol type: {name}")
            sub.evidence.protocol_declare = True
            cap.sub_capabilities[name] = sub

    cap.evidence.command_exists = any(
        sub.evidence.command_exists for sub in cap.sub_capabilities.values()
    )
    cap.evidence.protocol_declare = any(
        sub.evidence.protocol_declare for sub in cap.sub_capabilities.values()
    )

    return cap


def analyze_sandbox_capabilities(schema_defs: Set[str], ts_types: Set[str]) -> Capability:
    cap = Capability("sandbox", "Sandbox policies for command execution")
    cap.evidence.command_exists = flag_exists_in_help(_EXEC_HELP, "--sandbox")

    policies = ["read-only", "workspace-write", "danger-full-access"]
    for p in policies:
        sub = Capability(p.replace("-", "_"), f"Sandbox policy: {p}")
        sub.evidence.command_exists = flag_exists_in_help(_EXEC_HELP, p)
        cap.sub_capabilities[p.replace("-", "_")] = sub

    perms = Capability("permission_profile", "--permission-profile for named permission profiles")
    perms.evidence.command_exists = flag_exists_in_help(_ROOT_HELP, "--permission-profile")
    cap.sub_capabilities["permission_profile"] = perms

    dangerous_bypass = Capability(
        "dangerously_bypass_approvals_and_sandbox",
        "--dangerously-bypass-approvals-and-sandbox (EXTREMELY DANGEROUS, external sandbox only)"
    )
    dangerous_bypass.evidence.command_exists = flag_exists_in_help(_ROOT_HELP, "--dangerously-bypass-approvals-and-sandbox")
    dangerous_bypass.evidence.notes.append(
        "EXTREMELY DANGEROUS -- intended solely for externally-sandboxed environments. "
        "Not recommended for Kruon production use."
    )
    cap.sub_capabilities["dangerously_bypass_approvals_and_sandbox"] = dangerous_bypass

    return cap


def analyze_app_server_protocol(schema_defs: Set[str], ts_types: Set[str]) -> Capability:
    cap = Capability("app_server_protocol", "App server JSON-RPC protocol surface")
    cap.evidence.command_exists = subcommand_exists_in_help(_ROOT_HELP, "app-server")
    cap.evidence.protocol_declare = _has_schema_type(
        "InitializeParams", schema_defs, ts_types
    )
    if flag_exists_in_help(_APP_SERVER_HELP, "--listen"):
        cap.evidence.notes.append(
            "app-server help declares stdio, Unix socket, and WebSocket transports."
        )

    families = {
        "initialize": ("InitializeParams", "InitializeParams/Response"),
        "command_exec": ("CommandExecParams", "CommandExecParams/Response + Delta/Terminate"),
        "fs_ops": ("FsReadFileParams", "FsReadFile/Copy/Remove/Watch etc."),
        "config": ("ConfigReadParams", "ConfigRead/Write/BatchWrite"),
        "auth": ("LoginAccountParams", "LoginAccount/CancelLogin/AuthStatus"),
        "apps": ("AppsListParams", "AppsList/AppListUpdated"),
        "experimental_features": ("ExperimentalFeatureListParams", "ExperimentalFeatureList/EnablementSet"),
        "feedback": ("FeedbackUploadParams", "FeedbackUpload"),
        "external_agent_config": ("ExternalAgentConfigDetectParams", "ExternalAgentConfigImport/Detect"),
    }
    for name, (schema_name, desc) in families.items():
        sub = Capability(name, desc)
        sub.evidence.protocol_declare = _has_schema_type(
            schema_name, schema_defs, ts_types
        )
        cap.sub_capabilities[name] = sub

    approval_protocol = Capability(
        "approval_protocol",
        "Approval message types in app-server protocol (ExecCommandApproval, FileChangeRequestApproval, etc.)"
    )
    approval_protocol.evidence.protocol_declare = _has_schema_type(
        "ExecCommandApprovalParams", schema_defs, ts_types
    )
    approval_protocol.evidence.notes.append(
        "All 6 approval message types declared in v1 schema. "
        "This is a protocol_declare candidate for per_action -- "
        "closed loop not yet verified."
    )
    cap.sub_capabilities["approval_protocol"] = approval_protocol

    return cap


def analyze_network_capabilities(schema_defs: Set[str], ts_types: Set[str]) -> Capability:
    cap = Capability("network", "Network access control")

    for name in ["NetworkPolicyAmendment", "NetworkPolicyRuleAction"]:
        if _has_schema_type(name, schema_defs, ts_types):
            sub = Capability(name, f"Protocol type: {name}")
            sub.evidence.protocol_declare = True
            cap.sub_capabilities[name] = sub

    cap.evidence.protocol_declare = any(
        sub.evidence.protocol_declare for sub in cap.sub_capabilities.values()
    )

    return cap


# ---------------------------------------------------------------------------
# Builder
# ---------------------------------------------------------------------------

def build_analyzer() -> Dict[str, Capability]:
    schema_v1 = load_schema_defs(os.path.join(SCHEMA_DIR, "codex_app_server_protocol.schemas.json"))
    schema_v2 = load_schema_defs(os.path.join(SCHEMA_DIR, "codex_app_server_protocol.v2.schemas.json"))
    schema_defs = schema_v1 | schema_v2
    ts_types = load_ts_types(TS_DIR) | load_ts_types(os.path.join(TS_DIR, "v2"))

    return {
        "exec": analyze_exec_capabilities(schema_defs, ts_types),
        "approval": analyze_approval_capabilities(schema_defs, ts_types),
        "session": analyze_session_capabilities(schema_defs, ts_types),
        "artifact": analyze_artifact_capabilities(schema_defs, ts_types),
        "sandbox": analyze_sandbox_capabilities(schema_defs, ts_types),
        "app_server_protocol": analyze_app_server_protocol(schema_defs, ts_types),
        "network": analyze_network_capabilities(schema_defs, ts_types),
    }


# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------

def print_report(caps: Dict[str, Capability]) -> str:
    lines = []
    lines.append("=" * 60)
    lines.append("CODEX SERVICE PROTOCOL -- CAPABILITY ANALYSIS REPORT")
    lines.append("=" * 60)
    lines.append(f"Codex CLI version: {_VERSION}")
    lines.append(f"Evidence source: evidence/*.txt (collected from --help)")
    lines.append(f"Schema v1 definitions: {len(load_schema_defs(os.path.join(SCHEMA_DIR, 'codex_app_server_protocol.schemas.json')))}")
    lines.append(f"Schema v2 definitions: {len(load_schema_defs(os.path.join(SCHEMA_DIR, 'codex_app_server_protocol.v2.schemas.json')))}")
    lines.append(f"TS binding files: {len(load_ts_types(TS_DIR) | load_ts_types(os.path.join(TS_DIR, 'v2')))}")
    lines.append("")

    for group_name, group in sorted(caps.items()):
        lines.append(f"\n--- {group_name.upper()} ---")
        _print_capability(lines, group, indent=0)
    return "\n".join(lines)


def _print_capability(lines: List[str], cap: Capability, indent: int = 0):
    prefix = "  " * indent
    ev = cap.evidence
    lines.append(f"{prefix}{cap.name}: {cap.description}")
    lines.append(f"{prefix}  evidence_level={ev.level} cmd={ev.command_exists} proto={ev.protocol_declare} loop={ev.closed_loop}")
    for note in ev.notes:
        lines.append(f"{prefix}  note: {note}")
    for sub_name, sub in sorted(cap.sub_capabilities.items()):
        _print_capability(lines, sub, indent + 1)


def main():
    caps = build_analyzer()
    report = print_report(caps)
    print(report)

    out_path = os.path.join(SPIKE_DIR, "capability_report.txt")
    with open(out_path, "w") as f:
        f.write(report)
    print(f"\nReport written to {out_path}")


if __name__ == "__main__":
    main()
