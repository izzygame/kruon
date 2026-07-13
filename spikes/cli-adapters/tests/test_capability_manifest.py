"""Capability manifest safe-rejection tests (W1 acceptance: "安全拒绝").

These prove the launch-plan builder refuses to encode anything that would
bypass approval / sandbox / permission checks, and that the frozen plan's
approval mode is honestly declared per adapter. No real CLI is spawned --
``build_launch_plan`` only constructs the argv; execution is a separate
two-key opt-in in probe.py that these tests never touch.
"""

from __future__ import annotations

import os
import sys
import tempfile
import unittest

# Bootstrap: make the spikes/cli-adapters dir importable regardless of CWD.
_ADAPTERS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _ADAPTERS not in sys.path:
    sys.path.insert(0, _ADAPTERS)

import capability_manifest as cm  # noqa: E402
import probe  # noqa: E402


def _ws() -> str:
    return tempfile.mkdtemp(prefix="kruon-test-ws-")


class TestSafeRejection(unittest.TestCase):
    """build_launch_plan / determine_approval_mode MUST refuse unsafe inputs."""

    def test_each_dangerous_flag_rejected_as_extra_arg(self):
        for flag in cm.DANGEROUS_FLAGS:
            with self.subTest(flag=flag):
                with self.assertRaises(cm.UnsafeLaunchPlanError):
                    cm.build_launch_plan("codex", task="t", workspace=_ws(), extra_args=[flag])
                with self.assertRaises(cm.UnsafeLaunchPlanError):
                    cm.build_launch_plan(
                        "claude", task="t", workspace=_ws(),
                        claude_permission_mode="manual", extra_args=[flag],
                    )

    def test_dangerous_flag_equals_form_rejected(self):
        # ``--flag=value`` must be caught too, not just the bare token.
        for flag in cm.DANGEROUS_FLAGS:
            with self.subTest(flag=flag):
                with self.assertRaises(cm.UnsafeLaunchPlanError):
                    cm.build_launch_plan(
                        "codex", task="t", workspace=_ws(), extra_args=[f"{flag}=1"]
                    )

    def test_dangerous_flag_rejected_in_codex_argv_construction(self):
        # A dangerous flag must not slip in via the task string either: the
        # task is passed after `--` so it is an argument, not a flag, but we
        # still assert the assembled argv never contains a bare dangerous token.
        plan = cm.build_launch_plan("codex", task="--dangerously-skip-permissions", workspace=_ws())
        for flag in cm.DANGEROUS_FLAGS:
            self.assertNotIn(flag, plan.argv)

    def test_forbidden_permission_mode_rejected(self):
        for mode in cm.FORBIDDEN_PERMISSION_MODES:  # bypassPermissions
            with self.subTest(mode=mode):
                with self.assertRaises(cm.UnsafeLaunchPlanError):
                    cm.determine_approval_mode("claude", claude_permission_mode=mode)
                with self.assertRaises(cm.UnsafeLaunchPlanError):
                    cm.build_launch_plan(
                        "claude", task="t", workspace=_ws(), claude_permission_mode=mode
                    )

    def test_invalid_codex_sandbox_rejected(self):
        with self.assertRaises(ValueError):
            cm.build_launch_plan("codex", task="t", workspace=_ws(), codex_sandbox="open-wide")

    def test_unknown_adapter_rejected(self):
        with self.assertRaises(ValueError):
            cm.build_launch_plan("gemini", task="t", workspace=_ws())

    def test_empty_task_rejected(self):
        with self.assertRaises(ValueError):
            cm.build_launch_plan("codex", task="   ", workspace=_ws())
        with self.assertRaises(ValueError):
            cm.build_launch_plan("claude", task="", workspace=_ws())


class TestPositiveLaunchPlans(unittest.TestCase):
    """Happy paths: the frozen argv is safe and the approval mode is honest."""

    def test_help_analysis_keeps_codex_exec_separate(self):
        help_text = "Usage: codex exec\n  --json\n  --sandbox <MODE>\n"
        observed = probe.analyze_help_for_approval("codex", help_text)
        self.assertFalse(observed["has_ask_for_approval"])
        self.assertTrue(observed["has_json"])

    def test_claude_permission_choices_are_all_extracted(self):
        help_text = (
            '  --permission-mode <mode>\n'
            '      (choices: "acceptEdits", "auto", "bypassPermissions", '
            '"manual", "dontAsk", "plan")\n'
            '  --prompt-suggestions\n'
        )
        self.assertEqual(
            probe._extract_permission_choices(help_text),
            ["acceptEdits", "auto", "bypassPermissions", "manual", "dontAsk", "plan"],
        )

    def test_codex_plan_shape_and_safety(self):
        plan = cm.build_launch_plan("codex", task="list files", workspace=_ws())
        self.assertEqual(plan.adapter, "codex")
        self.assertEqual(plan.approval_mode, "sandbox_policy_only")
        self.assertEqual(plan.tool_name, "codex")
        # Required safe-construction tokens.
        for token in ("codex", "exec", "--json", "-s", "workspace-write",
                      "--skip-git-repo-check", "-"):
            self.assertIn(token, plan.argv)
        # Task is sent on stdin so prompt text cannot become a CLI option.
        self.assertEqual(plan.argv[-1], "-")
        self.assertEqual(plan.stdin_payload, "list files")
        # No dangerous bypass flag is ever emitted.
        for flag in cm.DANGEROUS_FLAGS:
            self.assertNotIn(flag, plan.argv)
        self.assertRegex(plan.fingerprint, r"^[0-9a-f]{16}$")

    def test_codex_default_sandbox_is_workspace_write(self):
        plan = cm.build_launch_plan("codex", task="t", workspace=_ws())
        # workspace-write is the safest mode that still edits the trusted ws.
        i = plan.argv.index("-s")
        self.assertEqual(plan.argv[i + 1], "workspace-write")

    def test_claude_manual_plan_targets_per_action(self):
        plan = cm.build_launch_plan(
            "claude", task="t", workspace=_ws(), claude_permission_mode="manual"
        )
        self.assertEqual(plan.approval_mode, "per_action")
        for token in ("claude", "-p", "stream-json", "--permission-mode", "manual"):
            self.assertIn(token, plan.argv)
        for flag in cm.DANGEROUS_FLAGS:
            self.assertNotIn(flag, plan.argv)

    def test_claude_permission_mode_matrix(self):
        # acceptEdits/plan -> sandbox_policy_only; auto/dontAsk -> none.
        cases = {
            "manual": "per_action",
            "acceptEdits": "sandbox_policy_only",
            "plan": "sandbox_policy_only",
            "auto": "none",
            "dontAsk": "none",
        }
        for mode, expected in cases.items():
            with self.subTest(mode=mode):
                self.assertEqual(
                    cm.determine_approval_mode("claude", claude_permission_mode=mode), expected
                )

    def test_codex_approval_is_always_sandbox_policy_only(self):
        # codex exec has no --ask-for-approval; the mode does not depend on args.
        self.assertEqual(cm.determine_approval_mode("codex"), "sandbox_policy_only")

    def test_fingerprint_is_deterministic(self):
        ws = _ws()
        p1 = cm.build_launch_plan("claude", task="say hello", workspace=ws,
                                  claude_permission_mode="manual")
        p2 = cm.build_launch_plan("claude", task="say hello", workspace=ws,
                                  claude_permission_mode="manual")
        self.assertEqual(p1.fingerprint, p2.fingerprint)

    def test_fingerprint_drift_on_task_change(self):
        ws = _ws()
        p1 = cm.build_launch_plan("claude", task="say hello", workspace=ws,
                                  claude_permission_mode="manual")
        p2 = cm.build_launch_plan("claude", task="say goodbye", workspace=ws,
                                  claude_permission_mode="manual")
        self.assertNotEqual(p1.fingerprint, p2.fingerprint)

    def test_fingerprint_drift_on_workspace_change(self):
        p1 = cm.build_launch_plan("claude", task="t", workspace=_ws(),
                                  claude_permission_mode="manual")
        p2 = cm.build_launch_plan("claude", task="t", workspace=_ws(),
                                  claude_permission_mode="manual")
        self.assertNotEqual(p1.fingerprint, p2.fingerprint)

    def test_fingerprint_drift_on_approval_mode_change(self):
        ws = _ws()
        p1 = cm.build_launch_plan("claude", task="t", workspace=ws,
                                  claude_permission_mode="manual")
        p2 = cm.build_launch_plan("claude", task="t", workspace=ws,
                                  claude_permission_mode="acceptEdits")
        self.assertNotEqual(p1.fingerprint, p2.fingerprint)

    def test_model_and_budget_wired_into_claude_argv(self):
        plan = cm.build_launch_plan(
            "claude", task="t", workspace=_ws(),
            claude_permission_mode="manual", model="claude-sonnet-5", max_budget_usd=0.25,
        )
        self.assertIn("--model", plan.argv)
        self.assertIn("claude-sonnet-5", plan.argv)
        self.assertIn("--max-budget-usd", plan.argv)
        self.assertIn("0.25", plan.argv)

    def test_to_dict_roundtrip_keys(self):
        plan = cm.build_launch_plan("codex", task="t", workspace=_ws())
        d = plan.to_dict()
        for key in ("adapter", "tool_name", "workspace", "task",
                    "approval_mode", "argv", "fingerprint"):
            self.assertIn(key, d)


class TestSnapshotsConsistent(unittest.TestCase):
    """The curated snapshots must agree with the code's own declarations."""

    def _load(self, name: str) -> cm.CapabilitySnapshot:
        return cm.load_snapshot(os.path.join(_ADAPTERS, "snapshots", f"{name}_capability.json"))

    def test_approval_mode_matches_determination(self):
        codex = self._load("codex")
        claude = self._load("claude")
        self.assertEqual(codex.approval_mode, "sandbox_policy_only")
        self.assertEqual(codex.approval_mode, cm.determine_approval_mode("codex"))
        self.assertEqual(claude.approval_mode, "per_action")
        self.assertEqual(
            cm.determine_approval_mode("claude", claude_permission_mode="manual"), "per_action"
        )

    def test_dangerous_bypass_marked_forbidden_and_in_constant(self):
        for name in ("codex", "claude"):
            with self.subTest(name=name):
                cap = self._load(name).capabilities.get("dangerous_bypass", {})
                self.assertTrue(cap.get("supported"), "dangerous bypass is documented")
                self.assertTrue(cap.get("forbidden_in_kruon"), "kruon forbids it")
                for flag in cap.get("flags", []):
                    self.assertIn(flag, cm.DANGEROUS_FLAGS)

    def test_snapshot_versions_present(self):
        for name in ("codex", "claude"):
            with self.subTest(name=name):
                snap = self._load(name)
                self.assertIsNotNone(snap.version)
                self.assertIn(snap.approval_mode, cm.APPROVAL_MODES)


if __name__ == "__main__":
    unittest.main()
