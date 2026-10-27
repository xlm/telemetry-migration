"""Compare Processor manifests with the Baseline and scan Harness runtime data."""

from __future__ import annotations

import json
from pathlib import Path

import yaml

from harness.common import ROOT, StepResult, read_jsonl

DISPOSITIONS = (
    "no-material-change",
    "clarification",
    "new-processing",
    "violation",
)
DISPLAY_LABELS = {
    "no-material-change": "no material change",
    "clarification": "clarification - reviewer confirms",
    "new-processing": "new processing - requires privacy review",
    "violation": "policy violation - requires remediation",
}


def compare_manifests(processor: str, *, root: Path = ROOT) -> tuple[list[dict], str]:
    baseline = _load_yaml(root / "manifests" / f"{processor}.job.yaml")
    manifest = _load_yaml(root / "manifests" / f"{processor}.processor.yaml")
    classification = _load_yaml(root / "manifests" / "classification.yaml")
    # The order in classification.yaml is the sensitivity order.
    sensitivity_order = list(classification.get("classifications", {}))
    findings = []

    def add(
        finding_type: str,
        topic: str | None,
        field: str | None,
        baseline_value,
        processor_value,
        disposition: str,
        message: str,
    ) -> None:
        findings.append(
            {
                "check": "manifest",
                "type": finding_type,
                "topic": topic,
                "field": field,
                "baseline": baseline_value,
                "processor": processor_value,
                "disposition": disposition,
                "message": message,
            }
        )

    for area in ("inputs", "outputs"):
        baseline_topics = {item["topic"]: item for item in baseline.get(area, [])}
        processor_topics = {item["topic"]: item for item in manifest.get(area, [])}
        for topic in sorted(processor_topics.keys() - baseline_topics.keys()):
            add(
                "new-topic",
                topic,
                None,
                None,
                topic,
                "new-processing",
                f"new {area[:-1]} topic {topic}",
            )
        for topic in sorted(baseline_topics.keys() - processor_topics.keys()):
            add(
                "removed-topic",
                topic,
                None,
                topic,
                None,
                "clarification",
                f"removed {area[:-1]} topic {topic}",
            )

        for topic in sorted(baseline_topics.keys() & processor_topics.keys()):
            baseline_topic = baseline_topics[topic]
            processor_topic = processor_topics[topic]
            baseline_fields = {field["name"]: field for field in baseline_topic.get("fields", [])}
            processor_fields = {field["name"]: field for field in processor_topic.get("fields", [])}
            for field in sorted(processor_fields.keys() - baseline_fields.keys()):
                classification_name = processor_fields[field].get("classification")
                is_output_location = (
                    area == "outputs" and classification_name == "sensitive-location"
                )
                add(
                    f"new-{area[:-1]}-field",
                    topic,
                    field,
                    None,
                    classification_name,
                    "violation" if is_output_location else "new-processing",
                    f"new {area[:-1]} field {field} on {topic}",
                )
            for field in sorted(baseline_fields.keys() - processor_fields.keys()):
                add(
                    "removed-field",
                    topic,
                    field,
                    baseline_fields[field],
                    None,
                    "clarification",
                    f"removed {area[:-1]} field {field} from {topic}",
                )
            for field in sorted(baseline_fields.keys() & processor_fields.keys()):
                old_field = baseline_fields[field]
                new_field = processor_fields[field]
                old_classification = old_field.get("classification")
                new_classification = new_field.get("classification")
                if old_classification != new_classification:
                    old_rank = sensitivity_order.index(old_classification)
                    new_rank = sensitivity_order.index(new_classification)
                    disposition = "violation" if new_rank < old_rank else "clarification"
                    direction = "loosened" if new_rank < old_rank else "tightened"
                    add(
                        "classification-change",
                        topic,
                        field,
                        old_classification,
                        new_classification,
                        disposition,
                        f"classification {direction} for {field}: "
                        f"{old_classification} to {new_classification}",
                    )
                if area == "inputs" and old_field.get("purpose") != new_field.get("purpose"):
                    add(
                        "purpose-change",
                        topic,
                        field,
                        old_field.get("purpose"),
                        new_field.get("purpose"),
                        "new-processing",
                        f"input purpose changed for {field}",
                    )
                if area == "outputs" and old_field.get("derived_from") != new_field.get(
                    "derived_from"
                ):
                    add(
                        "derived-from-change",
                        topic,
                        field,
                        old_field.get("derived_from"),
                        new_field.get("derived_from"),
                        "clarification",
                        f"derived_from changed for {field}",
                    )

    baseline_outputs = {topic["topic"]: topic for topic in baseline.get("outputs", [])}
    processor_outputs = {topic["topic"]: topic for topic in manifest.get("outputs", [])}
    for topic in sorted(baseline_outputs.keys() & processor_outputs.keys()):
        old_destinations = set(baseline_outputs[topic].get("downstream", []))
        new_destinations = set(processor_outputs[topic].get("downstream", []))
        for destination in sorted(new_destinations - old_destinations):
            add(
                "new-downstream",
                topic,
                destination,
                None,
                destination,
                "new-processing",
                f"new Downstream System {destination} on {topic}",
            )
        for destination in sorted(old_destinations - new_destinations):
            add(
                "removed-downstream",
                topic,
                destination,
                destination,
                None,
                "clarification",
                f"removed Downstream System {destination} from {topic}",
            )

    old_retention = baseline.get("retention_days")
    new_retention = manifest.get("retention_days")
    if old_retention != new_retention:
        increased = new_retention is not None and (
            old_retention is None or new_retention > old_retention
        )
        add(
            "retention-increased" if increased else "retention-decreased",
            None,
            "retention_days",
            old_retention,
            new_retention,
            "new-processing" if increased else "clarification",
            f"retention_days {'increased' if increased else 'decreased'} "
            f"from {old_retention} to {new_retention}",
        )

    for category in ("forbidden", "hashed"):
        old_entries = set(baseline.get("logging", {}).get(category, []))
        new_entries = set(manifest.get("logging", {}).get(category, []))
        for field in sorted(old_entries - new_entries):
            add(
                f"logging-{category}-removed",
                None,
                field,
                field,
                None,
                "violation",
                f"logging {category} restriction removed for {field}",
            )
        for field in sorted(new_entries - old_entries):
            add(
                f"logging-{category}-added",
                None,
                field,
                None,
                field,
                "clarification",
                f"logging {category} restriction added for {field}",
            )

    if baseline.get("version") != manifest.get("version"):
        add(
            "version-change",
            None,
            "version",
            baseline.get("version"),
            manifest.get("version"),
            "no-material-change",
            "manifest version changed",
        )
    return findings, _worst_disposition(item["disposition"] for item in findings)


def run_privacy(
    processor: str,
    *,
    root: Path = ROOT,
    work_dir: Path | None = None,
) -> StepResult:
    work_dir = work_dir or root / "target" / "gate" / processor
    manifest_findings, _manifest_disposition = compare_manifests(processor, root=root)
    manifest = _load_yaml(root / "manifests" / f"{processor}.processor.yaml")
    topics = manifest.get("outputs", [])
    standard_topic = next((topic for topic in topics if not topic["topic"].endswith(".dlq")), None)
    dlq_topic = next((topic for topic in topics if topic["topic"].endswith(".dlq")), None)
    runtime_findings = _scan_runtime(
        processor,
        root,
        work_dir,
        standard_topic,
        dlq_topic,
        manifest_findings,
    )
    findings = [*manifest_findings, *runtime_findings]
    disposition = _worst_disposition(item["disposition"] for item in findings)
    status = (
        "red"
        if DISPOSITIONS.index(disposition) >= DISPOSITIONS.index("new-processing")
        else "green"
    )
    counts = {
        "findings": len(findings),
        "manifest_findings": len(manifest_findings),
        "runtime_findings": len(runtime_findings),
        "disposition": disposition,
        "label": DISPLAY_LABELS[disposition],
    }
    return StepResult(
        "Privacy",
        status,
        f"disposition {disposition}, {len(findings)} findings",
        counts,
        findings,
    )


def _scan_runtime(
    processor: str,
    root: Path,
    work_dir: Path,
    standard_topic: dict | None,
    dlq_topic: dict | None,
    manifest_findings: list[dict],
) -> list[dict]:
    aggregated = {}
    for fixture in sorted((root / "fixtures" / processor).glob("*.jsonl")):
        fixture_records, _ = read_jsonl(fixture)
        locations = [
            record["location"]
            for record in fixture_records
            if isinstance(record.get("location"), dict)
            and record["location"].get("lat") is not None
            and record["location"].get("lon") is not None
        ]
        coordinates = {
            coordinate
            for location in locations
            for coordinate in (location["lat"], location["lon"])
            if isinstance(coordinate, (int, float)) and not isinstance(coordinate, bool)
        }
        vehicle_ids = {
            record["vehicle_id"]
            for record in fixture_records
            if isinstance(record.get("vehicle_id"), str) and record["vehicle_id"]
        }

        for output_path, topic_manifest in (
            (work_dir / f"{fixture.stem}.out.jsonl", standard_topic),
            (work_dir / f"{fixture.stem}.dlq.jsonl", dlq_topic),
        ):
            if topic_manifest is None:
                continue
            topic = topic_manifest["topic"]
            declared = {field["name"]: field for field in topic_manifest.get("fields", [])}
            sensitive_topic = any(
                field.get("classification") == "sensitive-location" for field in declared.values()
            )
            records, _ = read_jsonl(output_path)
            for record in records:
                for field, value in record.items():
                    if field not in declared:
                        _aggregate(
                            aggregated,
                            "undeclared-output-field",
                            topic,
                            field,
                            fixture.stem,
                            value,
                            f"field {field}, not declared on topic {topic}",
                        )
                    if _contains_location(value, locations, coordinates) and (
                        not sensitive_topic or field not in declared
                    ):
                        classification = declared.get(field, {}).get(
                            "classification", "sensitive-location"
                        )
                        unchanged = not any(
                            finding.get("topic") == topic and finding.get("field") == field
                            for finding in manifest_findings
                        )
                        message = (
                            f"field {field}, classification {classification}, "
                            f"not permitted on topic {topic}"
                        )
                        if unchanged:
                            message += ", Baseline unchanged"
                        _aggregate(
                            aggregated,
                            "location-in-output",
                            topic,
                            field,
                            fixture.stem,
                            value,
                            message,
                        )

        log_path = work_dir / f"{fixture.stem}.log.jsonl"
        if not log_path.is_file():
            continue
        try:
            log_lines = log_path.read_text(encoding="utf-8").splitlines()
        except OSError:
            continue
        for line in log_lines:
            location_leak = any(
                form in line
                for location in locations
                for coordinate in (location["lat"], location["lon"])
                for form in _number_forms(coordinate)
            )
            if location_leak:
                _aggregate(
                    aggregated,
                    "location-in-log",
                    "log",
                    "location",
                    fixture.stem,
                    line,
                    "log line contains a fixture location coordinate",
                )
            if any(vehicle_id in line for vehicle_id in vehicle_ids):
                _aggregate(
                    aggregated,
                    "raw-vehicle-id-in-log",
                    "log",
                    "vehicle_id",
                    fixture.stem,
                    line,
                    "log line contains a raw fixture vehicle_id",
                )

    return [
        {
            "check": "runtime",
            "type": finding_type,
            "topic": topic,
            "field": field,
            "baseline": None,
            "processor": None,
            "disposition": "violation",
            "message": item["message"],
            "count": item["count"],
            "fixture": item["fixture"],
            "example": item["example"],
        }
        for (finding_type, topic, field), item in aggregated.items()
    ]


def _aggregate(
    aggregated: dict,
    finding_type: str,
    topic: str,
    field: str,
    fixture: str,
    example,
    message: str,
) -> None:
    key = (finding_type, topic, field)
    if key not in aggregated:
        aggregated[key] = {
            "count": 0,
            "fixture": fixture,
            "example": example,
            "message": message,
        }
    aggregated[key]["count"] += 1


def _contains_location(value, locations: list[dict], coordinates: set) -> bool:
    if any(value == location for location in locations):
        return True
    if isinstance(value, (int, float)) and not isinstance(value, bool) and value in coordinates:
        return True
    if isinstance(value, dict):
        return any(_contains_location(item, locations, coordinates) for item in value.values())
    if isinstance(value, list):
        return any(_contains_location(item, locations, coordinates) for item in value)
    return False


def _number_forms(value) -> set[str]:
    return {repr(float(value)), json.dumps(value)}


def _load_yaml(path: Path) -> dict:
    return yaml.safe_load(path.read_text(encoding="utf-8")) or {}


def _worst_disposition(dispositions) -> str:
    return max(dispositions, key=DISPOSITIONS.index, default="no-material-change")
