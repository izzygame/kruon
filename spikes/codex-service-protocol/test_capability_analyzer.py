"""
test_capability_analyzer.py -- Pure stdlib unittests for capability_analyzer.

All tests are offline (no network, no model inference). They verify:
  - Schema/TS loading works
  - Evidence levels are correctly assigned
  - All known capabilities are present
  - Unknown capabilities remain unknown/unverified
  - exec help does NOT contain --ask-for-approval
  - All closed_loop = false
  - Approval protocol_declare is based on actual schema types
  - decision.md recommends sandbox_policy_only
  - No dangerous bypass in any recommended command
"""

import json
import os
import tempfile
import unittest

import capability_analyzer as ca


class TestEvidenceLoading(unittest.TestCase):
    def test_evidence_dir_exists(self):
        self.assertTrue(os.path.isdir(ca.EVIDENCE_DIR))

    def test_version_loaded(self):
        v = ca.read_evidence_text("codex-version.txt")
        self.assertTrue(v.startswith("codex-cli "), f"Unexpected version: {v}")

    def test_root_help_loaded(self):
        text = ca.read_evidence_text("codex-help-root.txt")
        self.assertIn("Usage: codex", text)

    def test_exec_help_loaded(self):
        text = ca.read_evidence_text("codex-help-exec.txt")
        self.assertIn("Usage: codex exec", text)

    def test_app_server_help_loaded(self):
        text = ca.read_evidence_text("codex-help-app-server.txt")
        self.assertIn("Usage: codex app-server", text)
        self.assertIn("--listen", text)

    def test_exec_server_help_loaded(self):
        text = ca.read_evidence_text("codex-help-exec-server.txt")
        self.assertIn("Usage: codex exec-server", text)
        self.assertIn("EXPERIMENTAL", text)


class TestExecHelpNoAskForApproval(unittest.TestCase):
    """Issue 1: --ask-for-approval appears in root codex --help but NOT in codex exec --help."""

    def test_ask_for_approval_in_root_help(self):
        root_help = ca.read_evidence_text("codex-help-root.txt")
        self.assertTrue(
            ca.flag_exists_in_help(root_help, "--ask-for-approval"),
            "--ask-for-approval should appear in root codex --help"
        )

    def test_ask_for_approval_not_in_exec_help(self):
        exec_help = ca.read_evidence_text("codex-help-exec.txt")
        self.assertFalse(
            ca.flag_exists_in_help(exec_help, "--ask-for-approval"),
            "--ask-for-approval must NOT appear in codex exec --help"
        )

    def test_exec_per_action_capability_is_not_command_available(self):
        capability = ca.build_analyzer()["exec"].sub_capabilities["per_action_approval"]
        self.assertFalse(capability.evidence.command_exists)
        self.assertEqual(capability.evidence.level, "unknown")


class TestSchemaLoading(unittest.TestCase):
    def test_load_schema_defs_returns_set(self):
        path = os.path.join(ca.SCHEMA_DIR, "codex_app_server_protocol.schemas.json")
        defs = ca.load_schema_defs(path)
        self.assertIsInstance(defs, set)
        self.assertGreater(len(defs), 0, "Should have at least some definitions")

    def test_load_schema_defs_missing_file(self):
        defs = ca.load_schema_defs("/nonexistent/path.json")
        self.assertEqual(defs, set())

    def test_load_schema_defs_invalid_json(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
            f.write("not json")
            tmp = f.name
        try:
            defs = ca.load_schema_defs(tmp)
            self.assertEqual(defs, set())
        finally:
            os.unlink(tmp)

    def test_load_ts_types_returns_set(self):
        types = ca.load_ts_types(ca.TS_DIR)
        self.assertIsInstance(types, set)
        self.assertGreater(len(types), 0, "Should have at least some TS types")

    def test_load_ts_types_nonexistent_dir(self):
        types = ca.load_ts_types("/nonexistent/path")
        self.assertEqual(types, set())


class TestEvidenceLevels(unittest.TestCase):
    def test_level_unknown(self):
        ev = ca.Evidence()
        self.assertEqual(ev.level, "unknown")

    def test_level_command_exists(self):
        ev = ca.Evidence(command_exists=True)
        self.assertEqual(ev.level, "command_exists")

    def test_level_protocol_declare(self):
        ev = ca.Evidence(command_exists=True, protocol_declare=True)
        self.assertEqual(ev.level, "protocol_declare")

    def test_level_closed_loop(self):
        ev = ca.Evidence(command_exists=True, protocol_declare=True, closed_loop=True)
        self.assertEqual(ev.level, "closed_loop")


class TestCapabilityBuilder(unittest.TestCase):
    def setUp(self):
        self.caps = ca.build_analyzer()

    def test_all_groups_present(self):
        expected = {"exec", "approval", "session", "artifact", "sandbox", "app_server_protocol", "network"}
        self.assertEqual(set(self.caps.keys()), expected)

    def test_exec_has_resume_and_review(self):
        exec_cap = self.caps["exec"]
        self.assertIn("resume", exec_cap.sub_capabilities)
        self.assertIn("review", exec_cap.sub_capabilities)

    def test_approval_has_sub_capabilities(self):
        approval = self.caps["approval"]
        self.assertGreater(len(approval.sub_capabilities), 0)
        for name, sub in approval.sub_capabilities.items():
            self.assertIn(sub.evidence.level, ["command_exists", "protocol_declare", "closed_loop", "unknown"])

    def test_session_has_cancel(self):
        session = self.caps["session"]
        self.assertIn("cancel", session.sub_capabilities)
        self.assertFalse(session.sub_capabilities["cancel"].evidence.command_exists)
        self.assertTrue(session.sub_capabilities["cancel"].evidence.protocol_declare)

    def test_network_is_protocol_only(self):
        network = self.caps["network"]
        self.assertFalse(network.evidence.command_exists)
        self.assertTrue(network.evidence.protocol_declare)

    def test_sandbox_has_policies(self):
        sandbox = self.caps["sandbox"]
        for policy in ["read_only", "workspace_write", "danger_full_access"]:
            self.assertIn(policy, sandbox.sub_capabilities)

    def test_app_server_protocol_has_families(self):
        asp = self.caps["app_server_protocol"]
        self.assertIn("initialize", asp.sub_capabilities)
        self.assertIn("command_exec", asp.sub_capabilities)
        self.assertIn("fs_ops", asp.sub_capabilities)


class TestAllClosedLoopFalse(unittest.TestCase):
    """Issue 2: All closed_loop must be false — no live tests were run."""

    def test_no_closed_loop_evidence(self):
        caps = ca.build_analyzer()
        for group_name, group in caps.items():
            self.assertFalse(group.evidence.closed_loop, f"{group_name} should not have closed_loop")
            for sub_name, sub in group.sub_capabilities.items():
                self.assertFalse(
                    sub.evidence.closed_loop,
                    f"{group_name}.{sub_name} should not have closed_loop"
                )


class TestApprovalProtocolDeclare(unittest.TestCase):
    """Issue 5: Approval protocol_declare must be based on actual schema types, not hardcoded true."""

    def setUp(self):
        self.caps = ca.build_analyzer()
        self.approval = self.caps["approval"]

    def test_exec_command_approval_protocol_declare(self):
        sub = self.approval.sub_capabilities.get("exec_command_approval")
        self.assertIsNotNone(sub)
        self.assertTrue(sub.evidence.protocol_declare,
                        "exec_command_approval should have protocol_declare=True (ExecCommandApprovalParams in schema)")

    def test_command_execution_request_approval_protocol_declare(self):
        sub = self.approval.sub_capabilities.get("command_execution_request_approval")
        self.assertIsNotNone(sub)
        self.assertTrue(sub.evidence.protocol_declare,
                        "command_execution_request_approval should have protocol_declare=True")

    def test_file_change_request_approval_protocol_declare(self):
        sub = self.approval.sub_capabilities.get("file_change_request_approval")
        self.assertIsNotNone(sub)
        self.assertTrue(sub.evidence.protocol_declare,
                        "file_change_request_approval should have protocol_declare=True (FileChangeRequestApprovalParams in schema)")

    def test_permissions_request_approval_protocol_declare(self):
        sub = self.approval.sub_capabilities.get("permissions_request_approval")
        self.assertIsNotNone(sub)
        self.assertTrue(sub.evidence.protocol_declare,
                        "permissions_request_approval should have protocol_declare=True (PermissionsRequestApprovalParams in schema)")

    def test_apply_patch_approval_protocol_declare(self):
        sub = self.approval.sub_capabilities.get("apply_patch_approval")
        self.assertIsNotNone(sub)
        self.assertTrue(sub.evidence.protocol_declare,
                        "apply_patch_approval should have protocol_declare=True")

    def test_tool_request_user_input_protocol_declare(self):
        sub = self.approval.sub_capabilities.get("tool_request_user_input")
        self.assertIsNotNone(sub)
        self.assertTrue(sub.evidence.protocol_declare,
                        "tool_request_user_input should have protocol_declare=True")

    def test_approval_not_hardcoded_command_exists(self):
        """Approval sub-capabilities should NOT have command_exists hardcoded true.
        --ask-for-approval is a root-level flag, not an exec flag."""
        for name, sub in self.approval.sub_capabilities.items():
            self.assertFalse(sub.evidence.command_exists,
                             f"{name} should NOT have command_exists=True (--ask-for-approval is root-level, not exec)")


class TestDecisionSandboxPolicyOnly(unittest.TestCase):
    """Issue 4: Current decision must be sandbox_policy_only."""

    def test_decision_recommends_sandbox_policy_only(self):
        path = os.path.join(ca.SPIKE_DIR, "decision.md")
        with open(path) as f:
            content = f.read()
        self.assertIn("sandbox_policy_only", content,
                      "decision.md must mention sandbox_policy_only")
        self.assertIn("Recommended strategy: `sandbox_policy_only`",
                      content,
                      "decision.md must recommend sandbox_policy_only as the current implementation baseline")


class TestNoDangerousBypassInRecommendations(unittest.TestCase):
    """Issue 3: dangerously-bypass-approvals-and-sandbox must not appear in any recommended command."""

    def test_decision_no_dangerous_bypass_in_recommended_command(self):
        path = os.path.join(ca.SPIKE_DIR, "decision.md")
        with open(path) as f:
            content = f.read()
        rec_section = content[content.find("## Recommendation"):]
        self.assertNotIn("--dangerously-bypass", rec_section,
                         "decision.md recommended command must not contain --dangerously-bypass-approvals-and-sandbox")

    def test_readme_no_dangerous_bypass(self):
        path = os.path.join(ca.SPIKE_DIR, "README.md")
        with open(path) as f:
            content = f.read()
        self.assertNotIn("dangerously-bypass", content,
                         "README.md must not mention --dangerously-bypass-approvals-and-sandbox")


class TestAppServerIsJsonRpc(unittest.TestCase):
    """Issue 2: app-server is JSON-RPC over stdio/unix/ws, not PTY."""

    def test_decision_mentions_json_rpc(self):
        path = os.path.join(ca.SPIKE_DIR, "decision.md")
        with open(path) as f:
            content = f.read()
        self.assertIn("JSON-RPC", content,
                      "decision.md should describe app-server as JSON-RPC")

    def test_decision_does_not_call_app_server_pty(self):
        path = os.path.join(ca.SPIKE_DIR, "decision.md")
        with open(path) as f:
            content = f.read()
        self.assertNotIn("PTY", content,
                         "decision.md should not describe app-server as PTY")


class TestApprovalSchemaDeclareOnly(unittest.TestCase):
    """Issue 4: Schema declares approval types as app-server per_action candidate,
    but closed loop is not verified."""

    def test_decision_acknowledges_schema_declare_not_closed_loop(self):
        path = os.path.join(ca.SPIKE_DIR, "decision.md")
        with open(path) as f:
            content = f.read()
        self.assertIn("not verified", content,
                      "decision.md must state closed loop is not verified")
        self.assertIn("protocol_declare", content,
                      "decision.md must reference protocol_declare evidence")


class TestUnknownCapabilities(unittest.TestCase):
    def test_unknown_if_no_evidence(self):
        cap = ca.Capability("fake", "Fake capability")
        self.assertEqual(cap.evidence.level, "unknown")

    def test_unknown_sub_if_no_evidence(self):
        cap = ca.Capability("test", "Test", sub_capabilities={
            "unknown_sub": ca.Capability("unknown_sub", "No evidence")
        })
        self.assertEqual(cap.sub_capabilities["unknown_sub"].evidence.level, "unknown")


class TestNameToPascal(unittest.TestCase):
    def test_simple(self):
        self.assertEqual(ca.name_to_pascal("exec_command_approval"), "ExecCommandApproval")

    def test_file_change_request_approval(self):
        self.assertEqual(ca.name_to_pascal("file_change_request_approval"), "FileChangeRequestApproval")

    def test_permissions_request_approval(self):
        self.assertEqual(ca.name_to_pascal("permissions_request_approval"), "PermissionsRequestApproval")

    def test_single_word(self):
        self.assertEqual(ca.name_to_pascal("approval"), "Approval")

    def test_empty(self):
        self.assertEqual(ca.name_to_pascal(""), "")


class TestReportGeneration(unittest.TestCase):
    def test_print_report_returns_string(self):
        caps = ca.build_analyzer()
        report = ca.print_report(caps)
        self.assertIsInstance(report, str)
        self.assertIn("CODEX SERVICE PROTOCOL", report)
        self.assertIn("evidence_level", report)

    def test_report_mentions_evidence_source(self):
        caps = ca.build_analyzer()
        report = ca.print_report(caps)
        self.assertIn("evidence/*.txt", report)


class TestMainFunction(unittest.TestCase):
    def test_main_writes_report_file(self):
        report_path = os.path.join(ca.SPIKE_DIR, "capability_report.txt")
        if os.path.exists(report_path):
            os.unlink(report_path)
        try:
            ca.main()
            self.assertTrue(os.path.exists(report_path))
            with open(report_path) as f:
                content = f.read()
            self.assertIn("CODEX SERVICE PROTOCOL", content)
        finally:
            if os.path.exists(report_path):
                os.unlink(report_path)


if __name__ == "__main__":
    unittest.main()
