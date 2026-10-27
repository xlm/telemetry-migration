import copy
import json
from pathlib import Path

import pytest
import yaml

from harness.privacy import compare_manifests, run_privacy

CLASSIFICATIONS = {
    "classifications": {
        "public": {},
        "internal-operational": {},
        "vehicle-identifying": {"log": "hashed-only"},
        "sensitive-location": {"log": "forbidden", "output": "forbidden-unless-listed"},
    }
}


def _manifest(kind):
    return {
        "name": "demo",
        "kind": kind,
        "version": 1,
        "inputs": [
            {
                "topic": "demo.input",
                "fields": [
                    {
                        "name": "source",
                        "classification": "internal-operational",
                        "purpose": "projection",
                    },
                    {
                        "name": "vehicle_id",
                        "classification": "vehicle-identifying",
                        "purpose": "projection",
                    },
                ],
            }
        ],
        "outputs": [
            {
                "topic": "demo.output",
                "downstream": ["warranty"],
                "fields": [
                    {
                        "name": "value",
                        "classification": "internal-operational",
                        "derived_from": ["source"],
                    }
                ],
            }
        ],
        "logging": {"forbidden": ["location"], "hashed": ["vehicle_id"]},
        "retention_days": 30,
    }


def _write_tree(root: Path, processor_manifest=None, baseline=None):
    manifests = root / "manifests"
    manifests.mkdir(parents=True, exist_ok=True)
    baseline = baseline or _manifest("job")
    processor_manifest = processor_manifest or _manifest("processor")
    (manifests / "demo.job.yaml").write_text(
        yaml.safe_dump(baseline, sort_keys=False), encoding="utf-8"
    )
    (manifests / "demo.processor.yaml").write_text(
        yaml.safe_dump(processor_manifest, sort_keys=False), encoding="utf-8"
    )
    (manifests / "classification.yaml").write_text(
        yaml.safe_dump(CLASSIFICATIONS, sort_keys=False), encoding="utf-8"
    )


def _mutate(case, baseline, processor):
    if case == "new-output-field":
        processor["outputs"][0]["fields"].append(
            {"name": "added", "classification": "public", "derived_from": ["source"]}
        )
    elif case == "new-sensitive-output-field":
        processor["outputs"][0]["fields"].append(
            {
                "name": "end_location",
                "classification": "sensitive-location",
                "derived_from": ["source"],
            }
        )
    elif case == "new-input-field":
        processor["inputs"][0]["fields"].append(
            {
                "name": "added",
                "classification": "public",
                "purpose": "projection",
            }
        )
    elif case == "removed-input-field":
        processor["inputs"][0]["fields"] = processor["inputs"][0]["fields"][1:]
    elif case == "removed-output-field":
        processor["outputs"][0]["fields"] = []
    elif case == "classification-loosened":
        processor["inputs"][0]["fields"][0]["classification"] = "public"
    elif case == "classification-tightened":
        processor["inputs"][0]["fields"][0]["classification"] = "vehicle-identifying"
    elif case == "purpose-change":
        processor["inputs"][0]["fields"][0]["purpose"] = "new-purpose"
    elif case == "derived-from-change":
        processor["outputs"][0]["fields"][0]["derived_from"] = ["vehicle_id"]
    elif case == "new-input-topic":
        processor["inputs"].append({"topic": "demo.extra-input", "fields": []})
    elif case == "removed-input-topic":
        processor["inputs"] = []
    elif case == "new-output-topic":
        processor["outputs"].append({"topic": "demo.extra-output", "downstream": [], "fields": []})
    elif case == "removed-output-topic":
        processor["outputs"] = []
    elif case == "new-downstream":
        processor["outputs"][0]["downstream"].append("service")
    elif case == "removed-downstream":
        processor["outputs"][0]["downstream"] = []
    elif case == "retention-increased":
        processor["retention_days"] = 31
    elif case == "retention-decreased":
        processor["retention_days"] = 29
    elif case == "logging-forbidden-removed":
        processor["logging"]["forbidden"] = []
    elif case == "logging-forbidden-added":
        processor["logging"]["forbidden"].append("vehicle_id")
    elif case == "logging-hashed-removed":
        processor["logging"]["hashed"] = []
    elif case == "logging-hashed-added":
        processor["logging"]["hashed"].append("campaign_id")
    elif case == "version-change":
        processor["version"] = 2


@pytest.mark.parametrize(
    ("case", "finding_type", "disposition"),
    [
        ("new-output-field", "new-output-field", "new-processing"),
        ("new-sensitive-output-field", "new-output-field", "violation"),
        ("new-input-field", "new-input-field", "new-processing"),
        ("removed-input-field", "removed-field", "clarification"),
        ("removed-output-field", "removed-field", "clarification"),
        ("classification-loosened", "classification-change", "violation"),
        ("classification-tightened", "classification-change", "clarification"),
        ("purpose-change", "purpose-change", "new-processing"),
        ("derived-from-change", "derived-from-change", "clarification"),
        ("new-input-topic", "new-topic", "new-processing"),
        ("removed-input-topic", "removed-topic", "clarification"),
        ("new-output-topic", "new-topic", "new-processing"),
        ("removed-output-topic", "removed-topic", "clarification"),
        ("new-downstream", "new-downstream", "new-processing"),
        ("removed-downstream", "removed-downstream", "clarification"),
        ("retention-increased", "retention-increased", "new-processing"),
        ("retention-decreased", "retention-decreased", "clarification"),
        ("logging-forbidden-removed", "logging-forbidden-removed", "violation"),
        ("logging-forbidden-added", "logging-forbidden-added", "clarification"),
        ("logging-hashed-removed", "logging-hashed-removed", "violation"),
        ("logging-hashed-added", "logging-hashed-added", "clarification"),
        ("version-change", "version-change", "no-material-change"),
    ],
)
def test_manifest_diff_finding_types_and_dispositions(tmp_path, case, finding_type, disposition):
    baseline = _manifest("job")
    processor = _manifest("processor")
    _mutate(case, baseline, processor)
    _write_tree(tmp_path, processor, baseline)
    findings, overall = compare_manifests("demo", root=tmp_path)
    finding = next(item for item in findings if item["type"] == finding_type)
    assert finding["disposition"] == disposition
    assert overall == disposition


def test_identical_manifests_have_no_findings_and_version_is_non_material(tmp_path):
    baseline = _manifest("job")
    processor = copy.deepcopy(baseline)
    processor["kind"] = "processor"
    processor["name"] = "other-name"
    _write_tree(tmp_path, processor, baseline)
    findings, overall = compare_manifests("demo", root=tmp_path)
    assert findings == []
    assert overall == "no-material-change"


def test_worst_manifest_disposition_wins(tmp_path):
    baseline = _manifest("job")
    processor = _manifest("processor")
    processor["inputs"][0]["fields"][0]["classification"] = "public"
    processor["version"] = 2
    _write_tree(tmp_path, processor, baseline)
    findings, overall = compare_manifests("demo", root=tmp_path)
    assert {item["disposition"] for item in findings} == {
        "violation",
        "no-material-change",
    }
    assert overall == "violation"


def _runtime(
    root: Path,
    records: list[dict],
    output: list[dict],
    log_lines: list[str] | None = None,
    *,
    fixtures: dict[str, list[dict]] | None = None,
    processor_manifest=None,
    baseline=None,
):
    processor_manifest = processor_manifest or _manifest("processor")
    baseline = baseline or _manifest("job")
    _write_tree(root, processor_manifest, baseline)
    fixture_dir = root / "fixtures" / "demo"
    fixture_dir.mkdir(parents=True, exist_ok=True)
    fixtures = fixtures or {"01-sample": records}
    work_dir = root / "work"
    work_dir.mkdir(parents=True, exist_ok=True)
    for name, events in fixtures.items():
        (fixture_dir / f"{name}.jsonl").write_text(
            "".join(json.dumps(item) + "\n" for item in events), encoding="utf-8"
        )
        (work_dir / f"{name}.out.jsonl").write_text(
            "".join(json.dumps(item) + "\n" for item in output), encoding="utf-8"
        )
        (work_dir / f"{name}.dlq.jsonl").write_text("", encoding="utf-8")
        if log_lines is not None:
            (work_dir / f"{name}.log.jsonl").write_text(
                "\n".join(log_lines) + "\n", encoding="utf-8"
            )
    return run_privacy("demo", root=root, work_dir=work_dir)


def _event():
    return {
        "vehicle_id": "VH-raw-example",
        "location": {"lat": 1.234567, "lon": -2.345678},
    }


def test_runtime_reports_undeclared_output_key(tmp_path):
    result = _runtime(tmp_path, [_event()], [{"unapproved": "safe"}])
    assert any(item["type"] == "undeclared-output-field" for item in result.findings)


def test_runtime_detects_location_object_and_coordinate(tmp_path):
    event = _event()
    location_result = _runtime(tmp_path, [event], [{"unapproved": event["location"]}])
    coordinate_result = _runtime(tmp_path, [event], [{"value": event["location"]["lat"]}])
    assert any(item["type"] == "location-in-output" for item in location_result.findings)
    assert any(item["type"] == "location-in-output" for item in coordinate_result.findings)


def test_sensitive_location_is_allowed_when_declared_on_topic(tmp_path):
    baseline = _manifest("job")
    processor = _manifest("processor")
    allowed_field = {
        "name": "location",
        "classification": "sensitive-location",
        "derived_from": ["source"],
    }
    baseline["outputs"][0]["fields"].append(copy.deepcopy(allowed_field))
    processor["outputs"][0]["fields"].append(copy.deepcopy(allowed_field))
    event = _event()
    result = _runtime(
        tmp_path,
        [event],
        [{"location": event["location"]}],
        processor_manifest=processor,
        baseline=baseline,
    )
    assert not any(item["type"] == "location-in-output" for item in result.findings)
    assert not any(item["type"] == "undeclared-output-field" for item in result.findings)


def test_runtime_detects_location_and_raw_vehicle_id_in_logs(tmp_path):
    event = _event()
    location_result = _runtime(
        tmp_path,
        [event],
        [],
        [f"lat={event['location']['lat']}"],
    )
    vehicle_result = _runtime(
        tmp_path,
        [event],
        [],
        [f"vehicle={event['vehicle_id']}"],
    )
    hashed_result = _runtime(
        tmp_path,
        [event],
        [],
        ["vehicle_id_hash=abcdef123456"],
    )
    assert any(item["type"] == "location-in-log" for item in location_result.findings)
    assert any(item["type"] == "raw-vehicle-id-in-log" for item in vehicle_result.findings)
    assert not any(item["check"] == "runtime" for item in hashed_result.findings)


def test_runtime_aggregates_repeated_location_leaks(tmp_path):
    event = _event()
    result = _runtime(
        tmp_path,
        [],
        [{"value": event["location"]["lat"]}],
        fixtures={"01-first": [event], "02-second": [event]},
    )
    findings = [item for item in result.findings if item["type"] == "location-in-output"]
    assert len(findings) == 1
    assert findings[0]["count"] == 2
    assert findings[0]["fixture"] == "01-first"


def test_undeclared_end_location_message_names_classification_and_unchanged_baseline(
    tmp_path,
):
    event = _event()
    result = _runtime(
        tmp_path,
        [event],
        [{"end_location": event["location"]}],
    )
    finding = next(item for item in result.findings if item["type"] == "location-in-output")
    assert (
        "field end_location, classification sensitive-location, not permitted on topic "
        "demo.output, Baseline unchanged"
    ) in finding["message"]
