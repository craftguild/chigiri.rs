"""Measure real-process reference output size, timing, determinism, and JSON validity."""

import hashlib
import json
import statistics
import subprocess
import time


def measure_reference(root, directory, binary):
    measurements = {}
    bodies = {}
    for label, options in (
        ("common_text", []),
        ("common_all_text", ["all"]),
        ("common_json", ["--format", "json"]),
        ("common_all_json", ["all", "--format", "json"]),
        ("directive_text", ["directives", "representation"]),
        ("directive_json", ["directives", "--format", "json"]),
        ("schema_text", ["--schema", str(root / "crates/mock/fixtures/blog.graphql")]),
        ("schema_all_text", ["all", "--schema", str(root / "crates/mock/fixtures/blog.graphql")]),
        ("schema_models_json", ["models", "--schema", str(root / "crates/mock/fixtures/blog.graphql"), "--format", "json"]),
        ("schema_json", ["--schema", str(root / "crates/mock/fixtures/blog.graphql"), "--format", "json"]),
        ("config_json", ["--config", str(directory / "mock.toml"), "--format", "json"]),
    ):
        times = []
        expected = None
        for attempt in range(3):
            output = directory / f"reference-{label}-{attempt}.out"
            started = time.monotonic()
            with output.open("wb") as stdout:
                result = subprocess.run([str(binary), "mock", "reference", *options], cwd=directory,
                                        stdout=stdout, stderr=subprocess.PIPE, check=True, timeout=10)
            times.append(time.monotonic() - started)
            if result.stderr:
                raise ValueError("The reference run emitted a diagnostic.")
            body = output.read_bytes()
            if expected is not None and expected != body:
                raise ValueError("The reference changed with the same input.")
            expected = body
            if label.endswith("json"):
                document = json.loads(body)
                if document["readiness_checked"] is not False:
                    raise ValueError("Reference document contract does not match.")
                if document["format"] == "chigiri-mock-reference/v13":
                    if document["config_executed"] is not False:
                        raise ValueError("Full reference document contract does not match.")
                elif document["format"] != "chigiri-mock-reference-topic/v9" or document["reference_format"] != "chigiri-mock-reference/v13":
                    raise ValueError("Reference topic document contract does not match.")
        bodies[label] = expected
        measurements[label] = {"bytes": len(expected), "sha256": hashlib.sha256(expected).hexdigest(),
                               "seconds": times, "median_seconds": statistics.median(times)}
        if label.endswith("text"):
            measurements[label]["lines"] = len(expected.splitlines())
    if len(bodies["common_text"].splitlines()) > 30 or b"\nDIRECTIVES\n" not in bodies["common_all_text"]:
        raise ValueError("The short entry in the reference does not match the full text.")
    if bodies["common_json"] != bodies["common_all_json"]:
        raise ValueError("The default JSON reference differs from the explicit all-topic JSON reference.")
    if b"@representation" not in bodies["directive_text"]:
        raise ValueError("Reference directive details do not match.")
    topic_models = json.loads(bodies["schema_models_json"])
    if topic_models["content"]["kind"] != "models" or not topic_models["content"]["items"]:
        raise ValueError("Reference schema-model topic does not match.")
    schema = json.loads(bodies["schema_json"])
    bundle = json.loads(bodies["config_json"])
    if schema["input_kind"] != "schema" or schema["mock_format"] is not None or bundle["input_kind"] != "bundle" or bundle["mock_format"] != "chigiri-mock/v11":
        raise ValueError("Reference input-format metadata does not match.")
    # Schema and bundle inputs intentionally differ in state metadata. Normalize those
    # fields before comparing their common reference content.
    if schema["state_configured"] is not False or bundle["state_configured"] is not True:
        raise ValueError("Reference state-input distinction does not match.")
    slots = {f"{phase}.{timing}" for phase in ("request", "operation", "field") for timing in ("before", "after")}
    for document, expected_enabled in ((schema, None), (bundle, False)):
        if len(document["hooks"]) != 6 or {hook["slot"] for hook in document["hooks"]} != slots:
            raise ValueError("The six slots in the reference do not match.")
        for hook in document["hooks"]:
            if hook["enabled"] is not expected_enabled:
                raise ValueError("The reference reports an unexpected hook-enabled state.")
            hook["enabled"] = None
        for name in ("ChigiriState", "ChigiriStatePatch"):
            matches = [item for item in document["api_types"] if item["id"] == name]
            generic = {"kind": "map", "value_type_id": "json", "string_keys": True}
            expected = generic if document is schema else {"kind": "object", "fields": [], "closed": True}
            if matches != [{"id": name, "shape": expected}]:
                raise ValueError("Reference generic/configured state type does not match.")
            matches[0]["shape"] = generic
        for key in ("input_kind", "mock_format", "config_executed", "state_configured"):
            document.pop(key)
    if schema != bundle:
        raise ValueError("Schema and configuration references do not match.")
    return measurements
