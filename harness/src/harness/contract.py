"""Validate Processor output records against schemas and semantic assertions."""

from __future__ import annotations

import json
from collections import Counter
from pathlib import Path

from jsonschema import Draft202012Validator, FormatChecker

from harness.common import PROCESSORS, ROOT, StepResult, read_jsonl


def validate_contract(
    processor: str,
    *,
    root: Path = ROOT,
    work_dir: Path | None = None,
) -> StepResult:
    spec = PROCESSORS[processor]
    work_dir = work_dir or root / "target" / "gate" / processor
    schema_path = root / spec.contract
    schema = json.loads(schema_path.read_text(encoding="utf-8"))
    validator = Draft202012Validator(schema, format_checker=FormatChecker())
    findings = []
    records_total = 0
    schema_failures = 0
    semantic_failures = 0
    for output_path in sorted(work_dir.glob("*.out.jsonl")):
        fixture = output_path.name.removesuffix(".out.jsonl")
        records, parse_errors = read_jsonl(output_path)
        findings.extend(
            {
                "type": "unparseable-line",
                "fixture": fixture,
                "message": error,
            }
            for error in parse_errors
        )
        records_total += len(records)
        key_counts = Counter(
            json.dumps(record.get(spec.key), sort_keys=True, separators=(",", ":"))
            for record in records
        )
        key_values = {
            json.dumps(record.get(spec.key), sort_keys=True, separators=(",", ":")): record.get(
                spec.key
            )
            for record in records
        }
        for record in records:
            key = record.get(spec.key)
            for error in validator.iter_errors(record):
                schema_failures += 1
                field = ".".join(str(part) for part in error.absolute_path) or None
                message = error.message
                if error.validator == "additionalProperties":
                    extras = sorted(set(record) - set(schema.get("properties", {})))
                    if extras:
                        field = extras[0]
                        message = f"additional property {field!r} is not allowed"
                findings.append(
                    {
                        "type": "schema",
                        "fixture": fixture,
                        "key": key,
                        "field": field,
                        "rule": error.validator,
                        "message": message,
                    }
                )
            if spec.semantic_assertions:
                semantic_failures += _charging_assertions(record, fixture, key, findings)
        for key_token, count in key_counts.items():
            if count > 1:
                semantic_failures += 1
                key = key_values[key_token]
                findings.append(
                    {
                        "type": "semantic",
                        "fixture": fixture,
                        "key": key,
                        "field": spec.key,
                        "rule": "one record per key",
                        "message": f"key {key!r} appears {count} times",
                    }
                )

    counts = {
        "records": records_total,
        "schema_failures": schema_failures,
        "semantic_failures": semantic_failures,
    }
    status = "red" if schema_failures or semantic_failures or findings else "green"
    summary = (
        f"{records_total} records, {schema_failures} schema failures, "
        f"{semantic_failures} semantic failures"
    )
    return StepResult("Contract", status, summary, counts, findings)


def _charging_assertions(record: dict, fixture: str, key, findings: list[dict]) -> int:
    violations = []
    start = record.get("start_ts")
    end = record.get("end_ts")
    if (
        isinstance(start, (int, float))
        and not isinstance(start, bool)
        and isinstance(end, (int, float))
        and not isinstance(end, bool)
        and start > end
    ):
        violations.append(("start_ts <= end_ts", "start_ts", "start_ts exceeds end_ts"))
    energy = record.get("energy_kwh")
    if isinstance(energy, (int, float)) and not isinstance(energy, bool) and energy < 0:
        violations.append(("energy_kwh >= 0", "energy_kwh", "energy_kwh is negative"))
    event_count = record.get("event_count")
    if (
        isinstance(event_count, (int, float))
        and not isinstance(event_count, bool)
        and event_count < 1
    ):
        violations.append(("event_count >= 1", "event_count", "event_count is below 1"))
    findings.extend(
        {
            "type": "semantic",
            "fixture": fixture,
            "key": key,
            "field": field,
            "rule": rule,
            "message": message,
        }
        for rule, field, message in violations
    )
    return len(violations)
