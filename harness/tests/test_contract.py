import json
from pathlib import Path

from harness.common import ROOT
from harness.contract import validate_contract


def _summary(**updates):
    record = {
        "session_id": "CS-00000001",
        "vehicle_id": "VH-00000001",
        "start_ts": 1,
        "end_ts": 2,
        "duration_s": 1,
        "start_soc_pct": 10,
        "end_soc_pct": 20,
        "energy_kwh": 1.0,
        "peak_power_kw": 2.0,
        "event_count": 1,
        "close_reason": "COMPLETE",
        "schema_version": "1",
    }
    return record | updates


def _validate(tmp_path: Path, records: list[dict]):
    tmp_path.mkdir(parents=True, exist_ok=True)
    (tmp_path / "fixture.out.jsonl").write_text(
        "".join(json.dumps(item) + "\n" for item in records), encoding="utf-8"
    )
    return validate_contract("charging-session", root=ROOT, work_dir=tmp_path)


def test_schema_failure_identifies_additional_property(tmp_path):
    result = _validate(tmp_path, [_summary(end_location={"lat": 1, "lon": 2})])
    finding = next(item for item in result.findings if item["type"] == "schema")
    assert finding["field"] == "end_location"
    assert "end_location" in finding["message"]
    assert result.counts["schema_failures"] == 1


def test_each_charging_semantic_assertion_is_reported(tmp_path):
    result = _validate(
        tmp_path,
        [
            _summary(start_ts=3, end_ts=2),
            _summary(session_id="CS-00000002", energy_kwh=-1),
            _summary(session_id="CS-00000003", event_count=0),
        ],
    )
    rules = {item["rule"] for item in result.findings if item["type"] == "semantic"}
    assert rules == {"start_ts <= end_ts", "energy_kwh >= 0", "event_count >= 1"}
    assert result.counts["semantic_failures"] == 3


def test_duplicate_processor_key_is_a_semantic_failure(tmp_path):
    duplicate = _summary()
    result = _validate(tmp_path, [duplicate, duplicate])
    assert result.counts["semantic_failures"] == 1
    assert any(item["rule"] == "one record per key" for item in result.findings)
