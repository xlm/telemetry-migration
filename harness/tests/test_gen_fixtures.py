"""Check fixture structure and scenario intent without producing Goldens."""

import itertools
import json
from collections import Counter, defaultdict
from pathlib import Path

import pytest
from jsonschema import Draft202012Validator, FormatChecker

from harness.gen_fixtures import (
    CHARGING_SCENARIOS,
    DEFAULT_SEED,
    MINUTE,
    OTA_SCENARIOS,
    ROOT,
    arrival_order,
    generate,
)


@pytest.fixture(scope="module")
def generated(tmp_path_factory):
    directory = tmp_path_factory.mktemp("fixtures")
    generate(directory)
    return directory


def records(directory: Path, name: str, scenario: str) -> list[dict]:
    return [
        json.loads(line)
        for line in (directory / name / f"{scenario}.jsonl").read_text().splitlines()
    ]


def validator(name: str) -> Draft202012Validator:
    schema = json.loads((ROOT / "schemas" / f"{name}.v1.schema.json").read_text())
    return Draft202012Validator(schema, format_checker=FormatChecker())


def test_regeneration_is_byte_identical(generated, tmp_path):
    generate(tmp_path, DEFAULT_SEED)
    expected = {
        Path(name) / f"{scenario}.jsonl"
        for name, scenarios in (
            ("charging-session", CHARGING_SCENARIOS),
            ("ota-status", OTA_SCENARIOS),
        )
        for scenario in scenarios
    }
    assert {path.relative_to(generated) for path in generated.rglob("*.jsonl")} == expected
    for path in expected:
        assert (generated / path).read_bytes() == (tmp_path / path).read_bytes()
        assert (generated / path).read_bytes() == (ROOT / "fixtures" / path).read_bytes()
    generate(tmp_path, DEFAULT_SEED + 1)
    assert (generated / "charging-session/01-normal.jsonl").read_bytes() != (
        tmp_path / "charging-session/01-normal.jsonl"
    ).read_bytes()


@pytest.mark.parametrize("scenario", CHARGING_SCENARIOS)
def test_charging_invariants(generated, scenario):
    events = records(generated, "charging-session", scenario)
    assert events == sorted(events, key=arrival_order)
    vehicles = {event["vehicle_id"] for event in events}
    sessions = {event.get("session_id") for event in events} - {None}
    assert 3 <= len(vehicles) <= 6
    assert 4 <= len(sessions) <= 12
    assert all(event["charging_state"] == "HEARTBEAT" for event in events[-len(vehicles) :])
    valid = validator("charging-event")
    for vehicle_id in vehicles:
        group = [event for event in events if event["vehicle_id"] == vehicle_id]
        timestamps = [event["event_ts"] for event in group]
        assert len(timestamps) == len(set(timestamps))
        heartbeat = group[-1]
        assert heartbeat["session_id"] is None
        assert heartbeat["event_ts"] == max(timestamps[:-1]) + 45 * MINUTE
        assert sum(event.get("charging_state") == "HEARTBEAT" for event in group) == 1
        seen = set()
        accepted = defaultdict(list)
        for event in group[:-1]:
            if not valid.is_valid(event) or event["seq"] in seen:
                continue
            seen.add(event["seq"])
            accepted[event["session_id"]].append(event)
        for session_events in accepted.values():
            terminals = [
                event
                for event in session_events
                if event["charging_state"] in {"COMPLETE", "UNPLUGGED"}
            ]
            if scenario == "09-idle-timeout":
                assert not terminals
                continue
            assert len(terminals) == 1
            terminal = terminals[0]
            before_close = [
                event for event in session_events if arrival_order(event) <= arrival_order(terminal)
            ]
            assert (
                max(e["event_ts"] for e in before_close) - min(e["event_ts"] for e in before_close)
                < 30 * MINUTE
            )
            assert terminal["ingest_ts"] < heartbeat["ingest_ts"]
    invalid = [event for event in events if not valid.is_valid(event)]
    assert bool(invalid) == (scenario == "07-malformed")
    located = sum(event["location"] is not None for event in events)
    assert 0.2 <= located / len(events) <= 0.45


@pytest.mark.parametrize("scenario", CHARGING_SCENARIOS[2:4])
def test_duplicate_keeps_session_and_seq(generated, scenario):
    events = records(generated, "charging-session", scenario)
    groups = defaultdict(list)
    for event in events:
        groups[event["vehicle_id"], event["seq"]].append(event)
    duplicates = [group for group in groups.values() if len(group) > 1]
    assert len(duplicates) == 4
    for first, duplicate in duplicates:
        assert first["session_id"] == duplicate["session_id"]
        assert first["ingest_ts"] < duplicate["ingest_ts"]
        assert (first["event_id"] == duplicate["event_id"]) == (scenario == "03-duplicate-same-seq")


def test_concurrent_sessions_overlap_per_vehicle(generated):
    events = records(generated, "charging-session", "02-concurrent")
    for vehicle_id in {event["vehicle_id"] for event in events}:
        group = [event for event in events if event["vehicle_id"] == vehicle_id]
        starts = [e["ingest_ts"] for e in group if e["charging_state"] == "PLUGGED_IN"]
        ends = [e["ingest_ts"] for e in group if e["charging_state"] in {"COMPLETE", "UNPLUGGED"}]
        assert len(starts) == 3
        assert max(starts) < min(ends)


def test_out_of_order_is_event_time_not_file_order(generated):
    events = records(generated, "charging-session", "05-out-of-order")
    for vehicle_id in {event["vehicle_id"] for event in events}:
        group = [event for event in events if event["vehicle_id"] == vehicle_id]
        assert any(a["event_ts"] > b["event_ts"] for a, b in itertools.pairwise(group))


def test_late_arrives_after_complete_without_duplicate_key(generated):
    events = records(generated, "charging-session", "06-late-after-close")
    closed = set()
    seen = set()
    late = []
    for event in events:
        key = event["vehicle_id"], event["session_id"]
        if key in closed:
            assert (event["vehicle_id"], event["seq"]) not in seen
            late.append(event)
        if event["charging_state"] == "COMPLETE":
            closed.add(key)
        seen.add((event["vehicle_id"], event["seq"]))
    assert len(late) == 4


def test_null_power_and_paused_intervals(generated):
    nulls = records(generated, "charging-session", "08-null-power")
    assert any(e["charging_state"] == "CHARGING" and e["power_kw"] is None for e in nulls)
    paused = records(generated, "charging-session", "10-paused-intervals")
    for session_id in {event["session_id"] for event in paused} - {None}:
        states = [e["charging_state"] for e in paused if e["session_id"] == session_id]
        assert states == ["PLUGGED_IN", "CHARGING", "PAUSED", "CHARGING", "COMPLETE"]


def test_replay_resends_a_whole_session(generated):
    events = records(generated, "charging-session", "11-replay")
    counts = Counter((e["vehicle_id"], e["seq"], e["event_id"]) for e in events)
    repeated = [e for e in events if counts[e["vehicle_id"], e["seq"], e["event_id"]] == 2]
    assert len(repeated) == 32
    sessions = {event["session_id"] for event in repeated}
    assert len(sessions) == 4
    assert all(count == 2 for count in Counter(e["event_id"] for e in repeated).values())
    assert all(
        counts[e["vehicle_id"], e["seq"], e["event_id"]] == 2
        for e in events
        if e["session_id"] in sessions
    )


def test_ota_scenarios(generated):
    valid = validator("ota-event")
    normal = records(generated, "ota-status", "01-normal")
    assert {e["status"] for e in normal} == {"DOWNLOADING", "INSTALLING", "SUCCEEDED", "FAILED"}
    variants = records(generated, "ota-status", "02-variant-status-strings")
    assert {e["status"].lower() for e in variants} == {
        "downloading",
        "dl",
        "download",
        "installing",
        "inst",
        "done",
        "complete",
        "success",
        "failed",
        "error",
    }
    assert any(e["progress_pct"] < 0 for e in variants)
    assert any(e["progress_pct"] > 100 for e in variants)
    assert all(valid.is_valid(event) for event in normal + variants)
    malformed = records(generated, "ota-status", "03-malformed")
    assert malformed[0]["status"] == "UNKNOWN"
    assert all(not valid.is_valid(event) for event in malformed[1:])
