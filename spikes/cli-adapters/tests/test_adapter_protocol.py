"""Adapter Protocol tests (W1 S1-03).

Verifies that:
1. The ABC enforces all 12 required methods.
2. Value objects construct and serialize correctly.
3. A minimal concrete adapter can be implemented.
4. The async variant also enforces all methods.
"""

from __future__ import annotations

import os
import sys
import unittest

_ADAPTERS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _ADAPTERS not in sys.path:
    sys.path.insert(0, _ADAPTERS)

import adapter_protocol as ap


class TestValueObjects(unittest.TestCase):
    """Value object construction and field access."""

    def test_tool_identity_defaults(self):
        ti = ap.ToolIdentity(name="codex")
        self.assertEqual(ti.name, "codex")
        self.assertIsNone(ti.version)
        self.assertEqual(ti.auth_state, "unknown")

    def test_tool_identity_full(self):
        ti = ap.ToolIdentity(name="claude", version="2.1.205", auth_state="authenticated")
        self.assertEqual(ti.name, "claude")
        self.assertEqual(ti.version, "2.1.205")
        self.assertEqual(ti.auth_state, "authenticated")

    def test_adapter_session_defaults(self):
        s = ap.AdapterSession(session_id="s1", adapter="codex")
        self.assertEqual(s.session_id, "s1")
        self.assertEqual(s.adapter, "codex")
        self.assertIsNone(s.pid)
        self.assertEqual(s.metadata, {})

    def test_approval_decision_approved(self):
        d = ap.ApprovalDecision(
            session_id="s1", event_id="e1", decision="approved"
        )
        self.assertEqual(d.session_id, "s1")
        self.assertEqual(d.event_id, "e1")
        self.assertEqual(d.decision, "approved")
        self.assertIsNone(d.modified_params)

    def test_approval_decision_modified(self):
        d = ap.ApprovalDecision(
            session_id="s1", event_id="e1", decision="modified",
            modified_params={"path": "safe.txt"},
        )
        self.assertEqual(d.decision, "modified")
        self.assertEqual(d.modified_params, {"path": "safe.txt"})

    def test_approval_decision_denied(self):
        d = ap.ApprovalDecision(
            session_id="s1", event_id="e1", decision="denied"
        )
        self.assertEqual(d.decision, "denied")

    def test_artifact_candidate_defaults(self):
        a = ap.ArtifactCandidate(path="/tmp/out.txt")
        self.assertEqual(a.path, "/tmp/out.txt")
        self.assertFalse(a.in_workspace)
        self.assertIsNone(a.kind)

    def test_artifact_candidate_requires_positive_workspace_check(self):
        a = ap.ArtifactCandidate(path="/trusted/out.txt", in_workspace=True)
        self.assertTrue(a.in_workspace)

    def test_artifact_candidate_out_of_workspace(self):
        a = ap.ArtifactCandidate(path="/etc/passwd", in_workspace=False)
        self.assertFalse(a.in_workspace)

    def test_observed_terminal_state_completed(self):
        o = ap.ObservedTerminalState(terminal_state="completed", exit_code=0)
        self.assertEqual(o.terminal_state, "completed")
        self.assertEqual(o.exit_code, 0)
        self.assertFalse(o.reconciled)

    def test_observed_terminal_state_unknown(self):
        o = ap.ObservedTerminalState(terminal_state="unknown")
        self.assertEqual(o.terminal_state, "unknown")
        self.assertIsNone(o.exit_code)

    def test_redacted_diagnostic_bundle(self):
        b = ap.RedactedDiagnosticBundle(
            adapter="codex", session_id="s1",
            event_count=10, parse_error_count=1, degraded_count=0,
            terminal_state="completed",
        )
        self.assertEqual(b.adapter, "codex")
        self.assertEqual(b.event_count, 10)
        self.assertEqual(b.parse_error_count, 1)
        self.assertEqual(b.errors, [])


class TestAdapterProtocolABC(unittest.TestCase):
    """The ABC must enforce all 12 abstract methods."""

    def test_cannot_instantiate_abc(self):
        with self.assertRaises(TypeError):
            ap.AdapterProtocol()  # type: ignore[abstract]

    def test_concrete_must_implement_all_methods(self):
        # Missing one method -> TypeError
        with self.assertRaises(TypeError):
            type("PartialAdapter", (ap.AdapterProtocol,), {})()


class TestMinimalConcreteAdapter(unittest.TestCase):
    """A minimal adapter implementing all 12 methods must be instantiable."""

    def test_minimal_adapter_works(self):
        class MinimalAdapter(ap.AdapterProtocol):
            def probe(self) -> ap.ToolIdentity:
                return ap.ToolIdentity(name="minimal", version="1.0.0")

            def capabilities(self, version=None) -> dict:
                return {"adapter": "minimal", "approval_mode": "none"}

            def prepare(self, task, workspace, policy=None) -> dict:
                return {"adapter": "minimal", "task": task, "workspace": workspace}

            def start(self, launch_plan) -> ap.AdapterSession:
                return ap.AdapterSession(session_id="s1", adapter="minimal")

            def send_input(self, session, input_data) -> None:
                pass

            def stream_events(self, session):
                return iter([])

            def respond_approval(self, session, decision) -> None:
                pass

            def cancel(self, session, deadline_seconds=10.0) -> None:
                pass

            def resume(self, session_ref):
                return None

            def collect_artifacts(self, session):
                return []

            def reconcile(self, session) -> ap.ObservedTerminalState:
                return ap.ObservedTerminalState(terminal_state="completed", exit_code=0)

            def diagnostics(self, session) -> ap.RedactedDiagnosticBundle:
                return ap.RedactedDiagnosticBundle(
                    adapter="minimal", session_id=session.session_id,
                    event_count=0, parse_error_count=0, degraded_count=0,
                )

        adapter = MinimalAdapter()
        self.assertIsInstance(adapter, ap.AdapterProtocol)

        # Probe
        ti = adapter.probe()
        self.assertEqual(ti.name, "minimal")
        self.assertEqual(ti.version, "1.0.0")

        # Capabilities
        caps = adapter.capabilities()
        self.assertEqual(caps["approval_mode"], "none")

        # Prepare
        plan = adapter.prepare("hello", "/tmp/ws")
        self.assertEqual(plan["task"], "hello")

        # Start
        session = adapter.start(plan)
        self.assertEqual(session.session_id, "s1")

        # Stream events
        events = list(adapter.stream_events(session))
        self.assertEqual(events, [])

        # Collect artifacts
        artifacts = adapter.collect_artifacts(session)
        self.assertEqual(artifacts, [])

        # Reconcile
        state = adapter.reconcile(session)
        self.assertEqual(state.terminal_state, "completed")

        # Diagnostics
        diag = adapter.diagnostics(session)
        self.assertEqual(diag.adapter, "minimal")
        self.assertEqual(diag.session_id, "s1")


class TestAsyncAdapterProtocolABC(unittest.TestCase):
    """The async ABC must also enforce all methods."""

    def test_cannot_instantiate_async_abc(self):
        with self.assertRaises(TypeError):
            ap.AsyncAdapterProtocol()  # type: ignore[abstract]


if __name__ == "__main__":
    unittest.main()
