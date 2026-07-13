"""Event parser tests: malformed JSON, unknown events, cancel/terminal
reconciliation, and secret redaction (W1 acceptance items).

All inputs are in-memory synthetic strings -- no real CLI, no files, no
network. These pin the parser's resilience contract: a bad line is a
ParseError (never a crash), an unknown type is a ``degraded`` event (never
a guessed terminal state), an abruptly-ended stream is ``unknown`` (never
coerced to completed), and obvious credential shapes are scrubbed.
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
from mini_jsonschema import validate as schema_validate  # noqa: E402

with open(os.path.join(_ADAPTERS, "normalized_event.schema.json"), encoding="utf-8") as _fh:
    _SCHEMA = json.load(_fh)


def _ok(event, err):
    """Assert exactly one of (event, err) is non-None for a single-line parse."""
    assert (event is None) ^ (err is None), f"expected event XOR err, got {event!r}, {err!r}"


class TestMalformedJSON(unittest.TestCase):
    """A non-JSON line yields a ParseError; parsing continues to a terminal."""

    def test_codex_bad_line_is_parse_error(self):
        ev, err = ep.parse_codex_line("this is not json {", line_no=7)
        self.assertIsNone(ev)
        self.assertIsNotNone(err)
        self.assertIn("malformed JSON", err.message)
        self.assertEqual(err.line_no, 7)
        # raw is redacted + truncated, and never None.
        self.assertIsInstance(err.raw, str)

    def test_codex_non_object_root_is_parse_error(self):
        for bad in ("[1,2,3]", "42", '"a string"', "true", "null"):
            with self.subTest(bad=bad):
                ev, err = ep.parse_codex_line(bad, line_no=1)
                self.assertIsNone(ev)
                self.assertIsNotNone(err)
                self.assertIn("not a JSON object", err.message)

    def test_claude_bad_line_is_parse_error(self):
        text = "not valid json here\n"
        results = list(ep.iter_claude_stream_json(text, approval_mode="per_action"))
        self.assertEqual(len(results), 1)
        ev, err = results[0]
        self.assertIsNone(ev)
        self.assertIsNotNone(err)
        self.assertIn("malformed JSON", err.message)

    def test_claude_non_object_root_is_parse_error(self):
        for bad in ("[1,2,3]", "42"):
            with self.subTest(bad=bad):
                results = list(ep.iter_claude_stream_json(bad, approval_mode="per_action"))
                ev, err = results[0]
                self.assertIsNone(ev)
                self.assertIsNotNone(err)
                self.assertIn("not a JSON object", err.message)

    def test_empty_line_yields_nothing(self):
        for line in ("", "   ", "  \n"):
            ev, err = ep.parse_codex_line(line, line_no=1)
            self.assertIsNone(ev)
            self.assertIsNone(err)

    def test_stream_continues_past_bad_line_to_terminal(self):
        # codex malformed fixture shape: good -> bad -> good -> turn.completed
        text = (
            '{"type":"session.created","payload":{"session_id":"s","cwd":"/w"}}\n'
            "this line is not valid json\n"
            '{"type":"turn.completed","payload":{"status":"completed"}}\n'
        )
        events, errors = ep.parse_stream(text, "codex", approval_mode="sandbox_policy_only")
        self.assertEqual(len(errors), 1)
        self.assertEqual(errors[0].line_no, 2)
        terminal, exit_code = ep.classify_terminal(events)
        self.assertEqual(terminal, "completed")
        self.assertEqual(exit_code, 0)


class TestUnknownEvents(unittest.TestCase):
    """An unrecognized ``type`` becomes a degraded event; raw is preserved."""

    def test_codex_unknown_type_is_degraded(self):
        line = '{"type":"new_future_event","payload":{"foo":"bar","whatever":[1,2,3]}}'
        ev, err = ep.parse_codex_line(line, line_no=1)
        _ok(ev, err)
        self.assertIsNone(err)
        self.assertEqual(ev.event_type, "unknown")
        self.assertTrue(ev.degraded)
        self.assertEqual(ev.phase, "degraded")
        self.assertEqual(ev.source["raw_type"], "new_future_event")
        # raw payload preserved for diagnostics.
        self.assertEqual(ev.content["payload"]["foo"], "bar")

    def test_claude_unknown_type_is_degraded(self):
        obj = {"type": "some_new_message_type", "data": "unknown to this parser version"}
        ev, err = ep.parse_claude_obj(obj, line_no=1, approval_mode="per_action")
        _ok(ev, err)
        self.assertIsNone(err)
        self.assertEqual(ev.event_type, "unknown")
        self.assertTrue(ev.degraded)
        self.assertEqual(ev.phase, "degraded")
        self.assertEqual(ev.source["raw_type"], "some_new_message_type")

    def test_unknown_event_is_schema_valid(self):
        # degraded=True + phase=degraded must satisfy the schema's allOf guard.
        for ev in (
            ep.parse_codex_line(
                '{"type":"new_future_event","payload":{"x":1}}', line_no=1
            )[0],
            ep.parse_claude_obj({"type": "weird"}, line_no=1, approval_mode="per_action")[0],
        ):
            with self.subTest(adapter=ev.adapter):
                errs = schema_validate(ev.to_dict(), _SCHEMA)
                self.assertEqual(errs, [], f"schema errors: {errs}")

    def test_unknown_event_does_not_force_terminal(self):
        # unknown is degraded, NOT terminal; a following turn.completed still wins.
        text = (
            '{"type":"new_future_event","payload":{"foo":"bar"}}\n'
            '{"type":"turn.completed","payload":{"status":"completed"}}\n'
        )
        events, errors = ep.parse_stream(text, "codex", approval_mode="sandbox_policy_only")
        self.assertEqual(errors, [])
        terminal, _ = ep.classify_terminal(events)
        self.assertEqual(terminal, "completed")


class TestCancelAndTerminalState(unittest.TestCase):
    """Abruptly-ended streams are ``unknown``; cancel resolution upgrades them."""

    def test_no_terminal_event_is_unknown(self):
        # codex cancel shape: session -> long tool call, no turn.completed.
        text = (
            '{"type":"session.created","payload":{"session_id":"s","cwd":"/w"}}\n'
            '{"type":"command_executed","payload":{"command":["sleep","30"],"exit_code":null}}\n'
        )
        events, errors = ep.parse_stream(text, "codex", approval_mode="sandbox_policy_only")
        self.assertEqual(errors, [])
        terminal, exit_code = ep.classify_terminal(events)
        self.assertEqual(terminal, "unknown")
        self.assertIsNone(exit_code)

    def test_claude_no_terminal_is_unknown(self):
        text = (
            '{"type":"system","subtype":"init","session_id":"s","cwd":"/w"}\n'
            '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash",'
            '"input":{"command":"sleep 30"}}]}}\n'
        )
        events, _ = ep.parse_stream(text, "claude", approval_mode="per_action")
        terminal, exit_code = ep.classify_terminal(events)
        self.assertEqual(terminal, "unknown")
        self.assertIsNone(exit_code)

    def test_conflicting_terminals_are_unknown_not_completed(self):
        # completed then failed -> must NOT be silently coerced to completed.
        text = (
            '{"type":"turn.completed","payload":{"status":"completed"}}\n'
            '{"type":"turn.completed","payload":{"status":"failed","exit_code":2}}\n'
        )
        events, _ = ep.parse_stream(text, "codex", approval_mode="sandbox_policy_only")
        terminal, _ = ep.classify_terminal(events)
        self.assertEqual(terminal, "unknown")

    def test_degraded_error_promotes_to_failed(self):
        # No terminal event, but a degraded `error` -> failed.
        text = '{"type":"error","payload":{"message":"boom"}}\n'
        events, _ = ep.parse_stream(text, "codex", approval_mode="sandbox_policy_only")
        terminal, exit_code = ep.classify_terminal(events)
        self.assertEqual(terminal, "failed")

    def test_resolve_cancel_state_matrix(self):
        # (observed, cancel_requested, responded_in_deadline) -> resolved
        cases = [
            ("completed", True, False, "completed"),    # clean CLI terminal trusted
            ("completed", False, False, "completed"),
            ("failed", True, False, "failed"),
            ("unknown", True, False, "forced_stop_required"),  # silent + deadline -> tear down
            ("unknown", True, True, "cancelled"),              # responded in time
            ("unknown", False, False, "unknown"),              # no cancel requested
            ("unknown", False, True, "unknown"),
        ]
        for observed, cancel, responded, expected in cases:
            with self.subTest(observed=observed, cancel=cancel, responded=responded):
                self.assertEqual(
                    ep.resolve_cancel_state(observed, cancel, responded), expected
                )

    def test_cancel_terminal_event_types_are_in_schema_enum(self):
        for state in ("completed", "failed", "cancelled", "forced_stop_required", "unknown"):
            with self.subTest(state=state):
                self.assertIn(state, ep.TERMINAL_STATES)

    def test_cancel_fixture_is_uncertain_and_upgrades(self):
        # The codex cancel fixture must observe unknown and escalate.
        fx_path = os.path.join(_ADAPTERS, "fixtures", "codex", "cancel_terminal.jsonl")
        with open(fx_path, encoding="utf-8") as fh:
            text = fh.read()
        events, _ = ep.parse_stream(text, "codex", approval_mode="sandbox_policy_only")
        observed, _ = ep.classify_terminal(events)
        self.assertEqual(observed, "unknown")
        self.assertEqual(
            ep.resolve_cancel_state(observed, cancel_requested=True,
                                    responded_within_deadline=False),
            "forced_stop_required",
        )


class TestSecretRedaction(unittest.TestCase):
    """Obvious credential shapes are scrubbed from text and from parsed events."""

    def test_known_secret_shapes_redacted(self):
        cases = [
            ("sk-abcdefABCDEFGH1234567890", "openai_key"),
            ("Bearer abc.def-ghi_xyz", "bearer_token"),
            ("ghp_abcdefghijklmnopqrstuvwxyz", "github_token"),
            ("AIzaSyABCDEFGHIJKLMNOPQRSTUVWXYZ1234", "google_api_key"),
            ("xoxb-1234567890-abcdef", "slack_token"),
        ]
        for secret, kind in cases:
            with self.subTest(kind=kind):
                out = ep.redact(f"token is {secret} here")
                self.assertNotIn(secret, out)
                self.assertIn(f"[REDACTED:{kind}]", out)

    def test_anthropic_key_is_redacted(self):
        # NOTE: sk-ant-* also matches the broader openai_key pattern (applied
        # first), so the label is generic -- but the secret IS removed. See
        # README "Unresolved risks". We assert the security property, not the
        # exact label, to avoid over-coupling to pattern ordering.
        secret = "sk-ant-abc123DEF456ghi789jkl012mno"
        out = ep.redact(f"ANTHROPIC_API_KEY={secret}")
        self.assertNotIn(secret, out)
        self.assertIn("[REDACTED:", out)

    def test_env_style_secret_redacted(self):
        out = ep.redact("export API_KEY=secretvalue123")
        self.assertNotIn("secretvalue123", out)
        self.assertIn("[REDACTED:env_secret]", out)
        # The variable NAME is preserved (only the value is scrubbed).
        self.assertIn("API_KEY", out)

    def test_env_style_secret_multiple_keywords(self):
        for name in ("MY_TOKEN", "DB_PASSWORD", "ACCESS_KEY", "CLIENT_CREDENTIAL"):
            with self.subTest(name=name):
                out = ep.redact(f"{name}=supersecret-value")
                self.assertNotIn("supersecret-value", out)
                self.assertIn("[REDACTED:env_secret]", out)

    def test_non_string_input_does_not_crash(self):
        for val in (None, 42, 3.14, ["a", "b"], {"k": "v"}):
            with self.subTest(val=val):
                out = ep.redact(val)
                self.assertIsInstance(out, str)

    def test_benign_text_is_not_mangled(self):
        # No false positives on ordinary commands / short sk- prefixes.
        for benign in ("ls -la", "git commit -m 'msg'", "sk-abc", "echo Bearer greeting"):
            with self.subTest(benign=benign):
                self.assertEqual(ep.redact(benign), benign)

    def test_redact_obj_recurses(self):
        obj = {
            "a": "sk-abcdefABCDEFGH1234567890",
            "nested": {"b": "ghp_abcdefghijklmnopqrstuvwxyz"},
            "list": ["xoxb-1234567890-abcdef"],
        }
        out = ep._redact_obj(obj)
        self.assertNotIn("sk-abcdefABCDEFGH1234567890", out["a"])
        self.assertIn("[REDACTED:openai_key]", out["a"])
        self.assertIn("[REDACTED:github_token]", out["nested"]["b"])
        self.assertIn("[REDACTED:slack_token]", out["list"][0])

    def test_secret_in_codex_payload_is_redacted_in_event(self):
        line = (
            '{"type":"message","payload":{"role":"assistant","content":'
            '[{"type":"text","text":"leaked sk-abcdefABCDEFGH1234567890 key"}]}}'
        )
        ev, err = ep.parse_codex_line(line, line_no=1)
        _ok(ev, err)
        self.assertIsNone(err)
        text = ev.content.get("text", "")
        self.assertNotIn("sk-abcdefABCDEFGH1234567890", text)
        self.assertIn("[REDACTED:openai_key]", text)
        # And in the raw_json provenance field.
        self.assertNotIn("sk-abcdefABCDEFGH1234567890", ev.source["raw_json"])

    def test_secret_in_claude_message_is_redacted_in_event(self):
        obj = {
            "type": "assistant",
            "message": {"content": [{"type": "text", "text": "key ghp_abcdefghijklmnopqrstuvwxyz"}]},
        }
        ev, err = ep.parse_claude_obj(obj, line_no=1, approval_mode="per_action")
        _ok(ev, err)
        self.assertIsNone(err)
        self.assertNotIn("ghp_abcdefghijklmnopqrstuvwxyz", ev.content.get("text") or "")
        self.assertIn("[REDACTED:github_token]", ev.content.get("text") or "")
        self.assertNotIn("ghp_abcdefghijklmnopqrstuvwxyz", ev.source["raw_json"])


class TestNormalizedEventShape(unittest.TestCase):
    """Every parser branch must emit a schema-valid normalized event."""

    def _codex_lines(self):
        return [
            '{"type":"session.created","payload":{"session_id":"s","cwd":"/w"}}',
            '{"type":"message","payload":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}',
            '{"type":"command_executed","payload":{"command":["ls"],"exit_code":0}}',
            '{"type":"file_update","payload":{"path":"src/a.md","change":"create"}}',
            '{"type":"turn.completed","payload":{"status":"completed","last_agent_message":"done"}}',
            '{"type":"error","payload":{"message":"boom"}}',
            '{"type":"new_future_event","payload":{"x":1}}',
        ]

    def _claude_objs(self):
        return [
            {"type": "system", "subtype": "init", "session_id": "s", "cwd": "/w", "model": "m"},
            {"type": "assistant", "message": {"content": [{"type": "text", "text": "hi"}]}},
            {"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Bash",
                                                          "input": {"command": "ls"}}]}},
            {"type": "user", "message": {"content": [{"type": "tool_result", "content": "out"}]}},
            {"type": "permission_request", "permission": {"tool_name": "Bash",
                                                          "input": {"command": "rm -rf x"}}},
            {"type": "result", "subtype": "success", "result": "ok", "is_error": False,
             "total_cost_usd": 0.01},
            {"type": "some_new_message_type", "data": "x"},
        ]

    def test_codex_events_schema_valid(self):
        for i, line in enumerate(self._codex_lines(), start=1):
            with self.subTest(i=i):
                ev, err = ep.parse_codex_line(line, line_no=i)
                _ok(ev, err)
                if ev is None:
                    continue
                errs = schema_validate(ev.to_dict(), _SCHEMA)
                self.assertEqual(errs, [], f"line {line}: {errs}")

    def test_claude_events_schema_valid(self):
        for i, obj in enumerate(self._claude_objs(), start=1):
            with self.subTest(i=i):
                ev, err = ep.parse_claude_obj(obj, line_no=i, approval_mode="per_action")
                _ok(ev, err)
                if ev is None:
                    continue
                errs = schema_validate(ev.to_dict(), _SCHEMA)
                self.assertEqual(errs, [], f"obj {obj}: {errs}")

    def test_approval_request_carries_fingerprint(self):
        obj = {"type": "permission_request", "permission": {"tool_name": "Write",
                                                            "input": {"path": "a.txt"}}}
        ev, _ = ep.parse_claude_obj(obj, line_no=1, approval_mode="per_action")
        self.assertEqual(ev.event_type, "approval.request")
        self.assertEqual(ev.phase, "waiting_approval")
        ar = ev.approval_request
        self.assertEqual(ar["kind"], "file_write")
        self.assertRegex(ar["fingerprint"], r"^[0-9a-f]{16}$")
        # Same input -> same fingerprint; different input -> different.
        ev2, _ = ep.parse_claude_obj(
            {"type": "permission_request", "permission": {"tool_name": "Write",
             "input": {"path": "a.txt"}}}, line_no=2, approval_mode="per_action")
        self.assertEqual(ar["fingerprint"], ev2.approval_request["fingerprint"])
        ev3, _ = ep.parse_claude_obj(
            {"type": "permission_request", "permission": {"tool_name": "Write",
             "input": {"path": "b.txt"}}}, line_no=3, approval_mode="per_action")
        self.assertNotEqual(ar["fingerprint"], ev3.approval_request["fingerprint"])

    def test_event_ids_are_deterministic(self):
        line = '{"type":"message","payload":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}'
        e1, _ = ep.parse_codex_line(line, line_no=4)
        e2, _ = ep.parse_codex_line(line, line_no=4)
        self.assertEqual(e1.event_id, e2.event_id)
        self.assertEqual(e1.event_id, "codex:4:message")


if __name__ == "__main__":
    unittest.main()
