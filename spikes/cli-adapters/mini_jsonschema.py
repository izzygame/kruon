"""Minimal JSON Schema (draft-07 subset) validator.

Pure Python 3 stdlib. Implements only the keywords this spike's
``normalized_event.schema.json`` uses, so the test suite can validate fixtures
and events without an external ``jsonschema`` dependency.

Supported keywords: ``type`` (string or list), ``enum``, ``const``,
``required``, ``properties``, ``additionalProperties`` (bool or schema),
``items``, ``minLength``, ``propertyNames``, ``allOf``, ``anyOf``, ``oneOf``, ``if``/``then``/
``else``. ``True``/``False`` schemas are supported.

This is deliberately not a complete implementation; it is enough to enforce the
adapter event contract and to make ``approval_mode`` enum drift fail loudly.
"""

from __future__ import annotations

from typing import Any, List, Union


def _check_type(instance: Any, t: Union[str, List[str]]) -> bool:
    types = t if isinstance(t, list) else [t]
    for one in types:
        if one == "object":
            if isinstance(instance, dict):
                return True
        elif one == "array":
            if isinstance(instance, list):
                return True
        elif one == "string":
            if isinstance(instance, str):
                return True
        elif one == "integer":
            # JSON Schema: booleans are not integers even though bool subclasses int.
            if isinstance(instance, int) and not isinstance(instance, bool):
                return True
        elif one == "number":
            if isinstance(instance, (int, float)) and not isinstance(instance, bool):
                return True
        elif one == "boolean":
            if isinstance(instance, bool):
                return True
        elif one == "null":
            if instance is None:
                return True
        else:
            # Unknown type keyword -> treat as permissive rather than crash.
            return True
    return False


def validate(instance: Any, schema: Any, path: str = "") -> List[str]:
    """Validate ``instance`` against ``schema``.

    Returns a list of human-readable error strings. An empty list means valid.
    Errors are collected (not raised) so callers can report every problem.
    """
    if schema is True:
        return []
    if schema is False:
        return [f"{path or '<root>'}: schema false (nothing allowed)"]
    if not isinstance(schema, dict):
        return [f"{path or '<root>'}: invalid schema (not a dict): {schema!r}"]

    errors: List[str] = []

    # type
    if "type" in schema:
        if not _check_type(instance, schema["type"]):
            got = type(instance).__name__
            errors.append(f"{path or '<root>'}: expected type {schema['type']!r}, got {got}")
            # Type mismatch makes most further checks meaningless.
            return errors

    # const
    if "const" in schema and instance != schema["const"]:
        errors.append(f"{path or '<root>'}: expected const {schema['const']!r}, got {instance!r}")

    # enum
    if "enum" in schema and instance not in schema["enum"]:
        errors.append(f"{path or '<root>'}: {instance!r} not in enum {schema['enum']!r}")

    # minLength
    if "minLength" in schema and isinstance(instance, str):
        if len(instance) < schema["minLength"]:
            errors.append(
                f"{path or '<root>'}: string length {len(instance)} < minLength {schema['minLength']}"
            )

    # Object keywords
    if isinstance(instance, dict):
        property_names = schema.get("propertyNames")
        if property_names is not None:
            for key in instance:
                key_path = f"{path}.<propertyName:{key}>" if path else f"<propertyName:{key}>"
                errors.extend(validate(key, property_names, key_path))
        for req in schema.get("required", []):
            if req not in instance:
                errors.append(f"{path or '<root>'}.{req}: required property missing")
        props = schema.get("properties", {})
        for key, val in instance.items():
            child_path = f"{path}.{key}" if path else key
            if key in props:
                errors.extend(validate(val, props[key], child_path))
            else:
                ap = schema.get("additionalProperties", True)
                if ap is False:
                    errors.append(f"{child_path}: additional property not allowed")
                elif isinstance(ap, dict):
                    errors.extend(validate(val, ap, child_path))

    # Array keywords
    if isinstance(instance, list) and "items" in schema:
        items_schema = schema["items"]
        for i, item in enumerate(instance):
            errors.extend(validate(item, items_schema, f"{path}[{i}]"))

    # Combinators
    for sub in schema.get("allOf", []):
        errors.extend(validate(instance, sub, path))

    any_of = schema.get("anyOf")
    if any_of is not None:
        if not any(not validate(instance, sub, path) for sub in any_of):
            errors.append(f"{path or '<root>'}: no anyOf branch matched")

    one_of = schema.get("oneOf")
    if one_of is not None:
        matches = sum(1 for sub in one_of if not validate(instance, sub, path))
        if matches != 1:
            errors.append(f"{path or '<root>'}: oneOf matched {matches} branches (expected 1)")

    # if / then / else
    if "if" in schema:
        if not validate(instance, schema["if"], path):
            if "then" in schema:
                errors.extend(validate(instance, schema["then"], path))
        else:
            if "else" in schema:
                errors.extend(validate(instance, schema["else"], path))

    return errors


def is_valid(instance: Any, schema: Any) -> bool:
    return not validate(instance, schema)
