"""The contracts reject extra fields, invalid bounds, and invalid state names."""

import copy
import json

import pytest
import yaml
from jsonschema import Draft202012Validator, FormatChecker

from harness.common import PROCESSORS
from harness.gen_fixtures import ROOT


def schema_validator(path):
    schema = json.loads((ROOT / path).read_text())
    Draft202012Validator.check_schema(schema)
    return Draft202012Validator(schema, format_checker=FormatChecker())


@pytest.mark.parametrize(
    "path", sorted((ROOT / "schemas").glob("*.json")) + sorted((ROOT / "contracts").glob("*.json"))
)
def test_draft_2020_12(path):
    schema = json.loads(path.read_text())
    assert schema["$schema"] == "https://json-schema.org/draft/2020-12/schema"
    Draft202012Validator.check_schema(schema)


@pytest.fixture
def charging_event():
    return json.loads(
        (ROOT / "fixtures/charging-session/01-normal.jsonl").read_text().splitlines()[0]
    )


@pytest.mark.parametrize(
    "field,value",
    [
        ("vehicle_id", "VH-1234567g"),
        ("vehicle_id", "VH-１２３４５６７８"),
        ("vehicle_id", " VH-12345678"),
        ("vehicle_id", "VH-12345678\n"),
        ("session_id", None),
        ("charging_state", "charging"),
        ("event_ts", 1.5),
        ("seq", "1"),
        ("soc_pct", -0.1),
        ("soc_pct", 100.1),
        ("power_kw", -0.1),
        ("schema_version", 1),
        ("event_id", "not-a-uuid"),
        ("location", {"lat": 91, "lon": 0}),
    ],
)
def test_charging_rejects_invalid_fields(charging_event, field, value):
    charging_event[field] = value
    assert not schema_validator("schemas/charging-event.v1.schema.json").is_valid(charging_event)


def test_nullable_fields_bounds_and_heartbeat(charging_event):
    valid = schema_validator("schemas/charging-event.v1.schema.json")
    assert valid.is_valid(charging_event)
    for value in [None, 0, 100]:
        charging_event["soc_pct"] = value
        charging_event["power_kw"] = value
        assert valid.is_valid(charging_event)
    charging_event["charging_state"] = "HEARTBEAT"
    assert not valid.is_valid(charging_event)
    charging_event["session_id"] = None
    assert valid.is_valid(charging_event)
    for field in valid.schema["required"]:
        missing = dict(charging_event)
        del missing[field]
        assert not valid.is_valid(missing)


@pytest.fixture(params=["charging-session", "ota-status"])
def output_record(request):
    if request.param == "ota-status":
        record = json.loads(
            (ROOT / "fixtures/ota-status/01-normal.jsonl").read_text().splitlines()[0]
        )
    else:
        # Schema examples only. These are not Job-generated Goldens.
        record = {
            "session_id": "CS-00000000",
            "vehicle_id": "VH-00000000",
            "start_ts": 0,
            "end_ts": 1000,
            "duration_s": 1,
            "start_soc_pct": None,
            "end_soc_pct": 100.0,
            "energy_kwh": 0.0,
            "peak_power_kw": 0.0,
            "event_count": 1,
            "close_reason": "COMPLETE",
            "schema_version": "1",
        }
    return schema_validator(f"contracts/{request.param}.v1.schema.json"), record


def test_contracts_are_closed_and_require_fields(output_record):
    valid, record = output_record
    assert valid.is_valid(record)
    for extra in ["location", "unexpected", "Location"]:
        assert not valid.is_valid({**record, extra: None})
    for field in valid.schema["required"]:
        missing = dict(record)
        del missing[field]
        assert not valid.is_valid(missing)
    for field in ["energy_kwh", "peak_power_kw", "duration_s", "event_count", "progress_pct"]:
        if field in record:
            changed = copy.deepcopy(record)
            changed[field] = -1
            assert not valid.is_valid(changed)
    state = "status" if "status" in record else "close_reason"
    assert not valid.is_valid({**record, state: record[state].lower()})


def test_manifests_cover_inputs_outputs_and_baselines():
    classifications = yaml.safe_load((ROOT / "manifests/classification.yaml").read_text())[
        "classifications"
    ]
    for name, input_name in [
        ("charging-session", "charging-event"),
        ("ota-status", "ota-event"),
        ("battery-health", "battery-event"),
    ]:
        baseline_path = ROOT / "manifests" / f"{name}.job.yaml"
        baseline = yaml.safe_load(baseline_path.read_text())
        assert baseline["logging"] == {"forbidden": ["location"], "hashed": ["vehicle_id"]}
        assert baseline["retention_days"] == 30
        for entries, schema_path in [
            (baseline["inputs"], f"schemas/{input_name}.v1.schema.json"),
            (baseline["outputs"][:1], f"contracts/{name}.v1.schema.json"),
        ]:
            fields = entries[0]["fields"]
            schema = json.loads((ROOT / schema_path).read_text())
            assert {field["name"] for field in fields} == set(schema["properties"])
        for topic in baseline["inputs"] + baseline["outputs"]:
            for field in topic["fields"]:
                assert field["classification"] in classifications
        for topic in baseline["outputs"]:
            assert "location" not in {field["name"] for field in topic["fields"]}


@pytest.mark.parametrize("name", sorted(PROCESSORS))
def test_manifest_kind_matches_filename(name):
    for kind in ["job", "processor"]:
        manifest = yaml.safe_load((ROOT / "manifests" / f"{name}.{kind}.yaml").read_text())
        assert manifest["kind"] == kind


def test_battery_contract_is_closed_and_bounded():
    valid = schema_validator("contracts/battery-health.v1.schema.json")
    record = {
        "event_id": "00000000-0000-4000-8000-000000000001",
        "vehicle_id": "VH-12345678",
        "battery_id": "BAT-00000001",
        "soh_pct": 80,
        "health": "HEALTHY",
        "event_ts": 10,
        "schema_version": "1",
    }
    assert valid.is_valid(record)
    for field in valid.schema["required"]:
        assert not valid.is_valid({key: value for key, value in record.items() if key != field})
    for changes in [
        {"soh_pct": -1},
        {"soh_pct": 101},
        {"health": "healthy"},
        {"location": None},
        {"schema_version": "99"},
    ]:
        assert not valid.is_valid({**record, **changes})
