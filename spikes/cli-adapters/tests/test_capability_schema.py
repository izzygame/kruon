"""Capability manifest schema validation tests (W1 S1-03).

Verifies that:
1. Both curated snapshots validate against capability_manifest.schema.json.
2. The schema rejects missing required fields.
3. The schema rejects invalid approval_mode values.
4. The schema rejects invalid evidence levels.
5. The schema rejects additional properties at root.
"""

from __future__ import annotations

import json
import os
import sys
import unittest

_ADAPTERS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _ADAPTERS not in sys.path:
    sys.path.insert(0, _ADAPTERS)

from mini_jsonschema import validate as schema_validate


def _load_schema() -> dict:
    path = os.path.join(_ADAPTERS, "capability_manifest.schema.json")
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


_SCHEMA = _load_schema()


def _load_snapshot(name: str) -> dict:
    path = os.path.join(_ADAPTERS, "snapshots", f"{name}_capability.json")
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


class TestSnapshotSchemaValid(unittest.TestCase):
    """Both curated snapshots must validate against the schema."""

    def test_codex_snapshot_valid(self):
        manifest = _load_snapshot("codex")
        errors = schema_validate(manifest, _SCHEMA)
        self.assertEqual(errors, [], f"codex snapshot schema errors: {errors}")

    def test_claude_snapshot_valid(self):
        manifest = _load_snapshot("claude")
        errors = schema_validate(manifest, _SCHEMA)
        self.assertEqual(errors, [], f"claude snapshot schema errors: {errors}")

    def test_codex_approval_mode_is_sandbox_policy_only(self):
        manifest = _load_snapshot("codex")
        self.assertEqual(manifest["approval_mode"], "sandbox_policy_only")

    def test_claude_approval_mode_is_per_action(self):
        manifest = _load_snapshot("claude")
        self.assertEqual(manifest["approval_mode"], "per_action")

    def test_codex_per_action_approval_evidence_is_inferred(self):
        manifest = _load_snapshot("codex")
        cap = manifest["capabilities"]["per_action_approval"]
        self.assertEqual(cap["evidence"], "inferred")
        self.assertFalse(cap["supported"])

    def test_claude_per_action_approval_evidence_is_inferred(self):
        manifest = _load_snapshot("claude")
        cap = manifest["capabilities"]["per_action_approval"]
        self.assertEqual(cap["evidence"], "inferred")
        self.assertTrue(cap["supported"])

    def test_codex_sandbox_policy_evidence_is_verified(self):
        manifest = _load_snapshot("codex")
        cap = manifest["capabilities"]["sandbox_policy"]
        self.assertEqual(cap["evidence"], "verified")

    def test_claude_sandbox_policy_evidence_is_inferred(self):
        manifest = _load_snapshot("claude")
        cap = manifest["capabilities"]["sandbox_policy"]
        self.assertEqual(cap["evidence"], "inferred")

    def test_both_have_schema_version(self):
        for name in ("codex", "claude"):
            with self.subTest(name=name):
                manifest = _load_snapshot(name)
                self.assertEqual(manifest.get("schema_version"), "1.0")

    def test_both_have_required_fields(self):
        required = ["schema_version", "adapter", "tool_name", "version",
                     "approval_mode", "interfaces", "capabilities", "notes"]
        for name in ("codex", "claude"):
            with self.subTest(name=name):
                manifest = _load_snapshot(name)
                for field in required:
                    self.assertIn(field, manifest, f"{name} missing {field}")


class TestSchemaRejection(unittest.TestCase):
    """The schema must reject invalid manifests."""

    def test_missing_approval_mode_rejected(self):
        manifest = _load_snapshot("codex").copy()
        del manifest["approval_mode"]
        errors = schema_validate(manifest, _SCHEMA)
        self.assertTrue(any("approval_mode" in e for e in errors),
                        f"expected error about missing approval_mode, got {errors}")

    def test_invalid_approval_mode_rejected(self):
        manifest = _load_snapshot("codex").copy()
        manifest["approval_mode"] = "per_session"
        errors = schema_validate(manifest, _SCHEMA)
        self.assertTrue(any("approval_mode" in e for e in errors),
                        f"expected error about invalid approval_mode, got {errors}")

    def test_missing_capabilities_rejected(self):
        manifest = _load_snapshot("codex").copy()
        del manifest["capabilities"]
        errors = schema_validate(manifest, _SCHEMA)
        self.assertTrue(any("capabilities" in e for e in errors),
                        f"expected error about missing capabilities, got {errors}")

    def test_missing_interfaces_rejected(self):
        manifest = _load_snapshot("codex").copy()
        del manifest["interfaces"]
        errors = schema_validate(manifest, _SCHEMA)
        self.assertTrue(any("interfaces" in e for e in errors),
                        f"expected error about missing interfaces, got {errors}")

    def test_missing_schema_version_rejected(self):
        manifest = _load_snapshot("codex").copy()
        del manifest["schema_version"]
        errors = schema_validate(manifest, _SCHEMA)
        self.assertTrue(any("schema_version" in e for e in errors),
                        f"expected error about missing schema_version, got {errors}")

    def test_invalid_evidence_level_rejected(self):
        manifest = _load_snapshot("codex").copy()
        cap = manifest["capabilities"]["sandbox_policy"]
        cap["evidence"] = "verified_help_text"  # not in enum
        errors = schema_validate(manifest, _SCHEMA)
        self.assertTrue(any("evidence" in e for e in errors),
                        f"expected error about invalid evidence, got {errors}")

    def test_additional_property_at_root_rejected(self):
        manifest = _load_snapshot("codex").copy()
        manifest["extra_field"] = "should not be allowed"
        errors = schema_validate(manifest, _SCHEMA)
        self.assertTrue(any("extra_field" in e for e in errors),
                        f"expected error about extra field, got {errors}")

    def test_invalid_interface_evidence_rejected(self):
        manifest = _load_snapshot("codex").copy()
        manifest["interfaces"][0]["evidence"] = "not_a_valid_evidence"
        errors = schema_validate(manifest, _SCHEMA)
        self.assertTrue(any("evidence" in e for e in errors),
                        f"expected error about invalid interface evidence, got {errors}")

    def test_capability_missing_supported_rejected(self):
        manifest = _load_snapshot("codex").copy()
        del manifest["capabilities"]["sandbox_policy"]["supported"]
        errors = schema_validate(manifest, _SCHEMA)
        self.assertTrue(any("supported" in e for e in errors),
                        f"expected error about missing supported, got {errors}")

    def test_capability_missing_evidence_rejected(self):
        manifest = _load_snapshot("codex").copy()
        del manifest["capabilities"]["sandbox_policy"]["evidence"]
        errors = schema_validate(manifest, _SCHEMA)
        self.assertTrue(any("evidence" in e for e in errors),
                        f"expected error about missing evidence, got {errors}")

    def test_empty_capability_name_rejected(self):
        manifest = json.loads(json.dumps(_load_snapshot("codex")))
        manifest["capabilities"][""] = {
            "supported": False,
            "evidence": "unverified",
        }
        errors = schema_validate(manifest, _SCHEMA)
        self.assertTrue(any("propertyName" in e for e in errors),
                        f"expected error about empty capability name, got {errors}")


class TestSchemaViaCapabilityManifestModule(unittest.TestCase):
    """The convenience functions in capability_manifest.py must work."""

    def test_validate_snapshot_file_codex(self):
        import capability_manifest as cm
        path = os.path.join(_ADAPTERS, "snapshots", "codex_capability.json")
        errors = cm.validate_snapshot_file(path)
        self.assertEqual(errors, [], f"codex snapshot validation errors: {errors}")

    def test_validate_snapshot_file_claude(self):
        import capability_manifest as cm
        path = os.path.join(_ADAPTERS, "snapshots", "claude_capability.json")
        errors = cm.validate_snapshot_file(path)
        self.assertEqual(errors, [], f"claude snapshot validation errors: {errors}")


if __name__ == "__main__":
    unittest.main()
