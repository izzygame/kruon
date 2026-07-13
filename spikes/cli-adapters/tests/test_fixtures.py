"""Dual-adapter fixture tests (W1 acceptance: "双适配器 fixtures").

Data-driven from ``fixtures/manifest.json``: every fixture for BOTH adapters
(codex + claude) is parsed and its declared expectations asserted --
terminal state, exit code, parse-error count, degraded count, event count,
approval-mode stamping, required event types, and schema validity. This also
pins the approval capability ASYMMETRY at the fixture level: codex exec runs
are always ``sandbox_policy_only`` (no per-action channel), while a Claude
Code manual run targets ``per_action``.
"""

from __future__ import annotations

import json
import os
import sys
import unittest

# Bootstrap: make the spikes/cli-adapters dir importable regardless of CWD.
_ADAPTERS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _ADAPTERS not in sys.path:
    sys.path.insert(0, _ADAPTERS)

import event_parser as ep  # noqa: E402
import probe  # noqa: E402  (for the self-test integration check)
from mini_jsonschema import validate as schema_validate  # noqa: E402

_FIXTURES = os.path.join(_ADAPTERS, "fixtures")
with open(os.path.join(_ADAPTERS, "normalized_event.schema.json"), encoding="utf-8") as _fh:
    _SCHEMA = json.load(_fh)
with open(os.path.join(_FIXTURES, "manifest.json"), encoding="utf-8") as _fh:
    _MANIFEST = json.load(_fh)


class TestDualAdapterFixtures(unittest.TestCase):
    """Every fixture in the manifest parses to its declared expectation."""

    def test_both_adapters_represented(self):
        adapters = {fx["adapter"] for fx in _MANIFEST["fixtures"]}
        self.assertEqual(adapters, {"codex", "claude"})

    def test_required_scenario_coverage(self):
        ids = {fx["id"] for fx in _MANIFEST["fixtures"]}
        for adapter in ("codex", "claude"):
            for scenario in ("success", "malformed", "unknown-event",
                             "nonzero-exit", "cancel-terminal"):
                with self.subTest(adapter=adapter, scenario=scenario):
                    self.assertIn(f"{adapter}-{scenario}", ids,
                                  f"missing {adapter} fixture for {scenario}")

    def test_every_fixture_meets_expectations(self):
        for fx in _MANIFEST["fixtures"]:
            with self.subTest(fx=fx["id"]):
                path = os.path.join(_FIXTURES, fx["path"])
                self.assertTrue(os.path.isfile(path), f"missing fixture file: {fx['path']}")
                with open(path, encoding="utf-8") as fh:
                    text = fh.read()
                exp = fx["expected"]

                events, errors = ep.parse_stream(
                    text, fx["adapter"], approval_mode=exp["approval_mode"]
                )
                terminal, exit_code = ep.classify_terminal(events)

                self.assertEqual(terminal, exp["terminal_state"],
                                 f"{fx['id']}: terminal {terminal!r} != {exp['terminal_state']!r}")
                if exp.get("exit_code") is not None:
                    self.assertEqual(exit_code, exp["exit_code"],
                                     f"{fx['id']}: exit_code {exit_code!r} != {exp['exit_code']!r}")
                else:
                    self.assertIsNone(exit_code, f"{fx['id']}: expected no exit code")

                self.assertEqual(len(errors), exp.get("parse_errors", 0),
                                 f"{fx['id']}: parse_errors {len(errors)} != {exp.get('parse_errors', 0)}")
                degraded = sum(1 for e in events if e.degraded)
                self.assertEqual(degraded, exp.get("degraded_count", 0),
                                 f"{fx['id']}: degraded {degraded} != {exp.get('degraded_count', 0)}")
                self.assertGreaterEqual(len(events), exp.get("events_min", 1),
                                        f"{fx['id']}: too few events {len(events)}")

                # approval_mode is a run property stamped on EVERY event honestly.
                for e in events:
                    self.assertEqual(e.approval_mode, exp["approval_mode"],
                                     f"{fx['id']}: event {e.event_id} approval_mode mismatch")

                # Required normalized event types are all emitted.
                emitted = {e.event_type for e in events}
                for et in exp.get("must_contain_event_types", []):
                    self.assertIn(et, emitted, f"{fx['id']}: missing event type {et!r}")

                # Every normalized event validates against the schema.
                for e in events:
                    errs = schema_validate(e.to_dict(), _SCHEMA)
                    self.assertEqual(errs, [], f"{fx['id']}: event {e.event_id} schema: {errs}")

    def test_approval_capability_asymmetry(self):
        """codex exec = sandbox_policy_only (no per-action channel);
        claude manual = per_action (target). This is the core W1 asymmetry."""
        by_id = {fx["id"]: fx for fx in _MANIFEST["fixtures"]}
        self.assertEqual(by_id["codex-success"]["expected"]["approval_mode"],
                         "sandbox_policy_only")
        self.assertEqual(by_id["claude-success"]["expected"]["approval_mode"],
                         "per_action")
        # Codex NEVER declares per_action (exec has no --ask-for-approval).
        for fx in _MANIFEST["fixtures"]:
            if fx["adapter"] == "codex":
                self.assertEqual(fx["expected"]["approval_mode"], "sandbox_policy_only",
                                 f"codex fixture {fx['id']} must be sandbox_policy_only")

    def test_cancel_fixtures_are_uncertain(self):
        """Abruptly-ended streams observe `unknown`, never completed."""
        for fx in _MANIFEST["fixtures"]:
            if not fx["expected"].get("uncertain"):
                continue
            with self.subTest(fx=fx["id"]):
                with open(os.path.join(_FIXTURES, fx["path"]), encoding="utf-8") as fh:
                    text = fh.read()
                events, _ = ep.parse_stream(
                    text, fx["adapter"], approval_mode=fx["expected"]["approval_mode"]
                )
                terminal, exit_code = ep.classify_terminal(events)
                self.assertEqual(terminal, "unknown")
                self.assertIsNone(exit_code)
                # Supervisor escalates a silent cancel past the deadline.
                self.assertEqual(
                    ep.resolve_cancel_state(terminal, cancel_requested=True,
                                            responded_within_deadline=False),
                    "forced_stop_required",
                )

    def test_fixtures_marked_synthetic(self):
        """Fixtures are hand-authored, not live captures (see README)."""
        for fx in _MANIFEST["fixtures"]:
            with self.subTest(fx=fx["id"]):
                self.assertTrue(fx.get("synthetic"), f"{fx['id']} must be flagged synthetic")

    def test_probe_self_test_passes(self):
        """The in-tree probe self-test must report all fixtures passing."""
        ok, results = probe.fixtures_self_test(verbose=False)
        self.assertTrue(ok, "probe.fixtures_self_test did not pass")
        self.assertEqual(len(results), len(_MANIFEST["fixtures"]))
        self.assertTrue(all(r["passed"] for r in results))


if __name__ == "__main__":
    unittest.main()
