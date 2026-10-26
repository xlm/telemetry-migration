"""Generate seeded, synthetic inputs without running a Job or producing Goldens.

Re-sends preserve dedup keys but get distinct event times, as required by the
fixture contract. Arrival order is always (ingest_ts, event_id).
"""

from __future__ import annotations

import copy
import json
import random
import uuid
from pathlib import Path

import click

DEFAULT_SEED = 20261026
ROOT = Path(__file__).resolve().parents[3]
MINUTE = 60_000
BASE_TS = 1_792_972_800_000
CHARGING_SCENARIOS = (
    "01-normal",
    "02-concurrent",
    "03-duplicate-same-seq",
    "04-duplicate-new-event-id",
    "05-out-of-order",
    "06-late-after-close",
    "07-malformed",
    "08-null-power",
    "09-idle-timeout",
    "10-paused-intervals",
    "11-replay",
)
OTA_SCENARIOS = ("01-normal", "02-variant-status-strings", "03-malformed")


def event_id(rng: random.Random) -> str:
    return str(uuid.UUID(int=rng.getrandbits(128), version=4))


def arrival_order(event: dict) -> tuple[int, str]:
    return event["ingest_ts"], event.get("event_id", "")


def charging_fixture(scenario: str, rng: random.Random) -> list[dict]:
    """Build four vehicles and eight sessions (twelve for concurrent sessions)."""
    events = []
    vehicles = [f"VH-{rng.getrandbits(32):08x}" for _ in range(4)]
    sessions_per_vehicle = 3 if scenario == "02-concurrent" else 2
    for vehicle_index, vehicle_id in enumerate(vehicles):
        vehicle_events = []
        for session_index in range(sessions_per_vehicle):
            session_id = f"CS-{vehicle_index * sessions_per_vehicle + session_index:08x}"
            start = BASE_TS + vehicle_index * 100
            start += session_index * (1_000 if scenario == "02-concurrent" else 10 * MINUTE)
            states = ["PLUGGED_IN", "CHARGING", "CHARGING", "COMPLETE"]
            if session_index % 2:
                states[-1] = "UNPLUGGED"
            if scenario == "10-paused-intervals":
                states = ["PLUGGED_IN", "CHARGING", "PAUSED", "CHARGING", "COMPLETE"]
            if scenario == "09-idle-timeout":
                states = states[:-1]
            for index, state in enumerate(states):
                timestamp = start + index * MINUTE
                power = round(rng.uniform(10, 100), 3)
                if state in {"PLUGGED_IN", "PAUSED", "COMPLETE", "UNPLUGGED"}:
                    power = 0.0
                if scenario == "08-null-power" and (session_index == 0 or index == 2):
                    power = None
                vehicle_events.append(
                    {
                        "event_id": event_id(rng),
                        "vehicle_id": vehicle_id,
                        "session_id": session_id,
                        "event_ts": timestamp,
                        "ingest_ts": timestamp + 10_000,
                        "charging_state": state,
                        "soc_pct": 20.0 + index * 5 if index else None,
                        "power_kw": power,
                        "location": (
                            {
                                "lat": round(rng.uniform(-60, 60), 6),
                                "lon": round(rng.uniform(-150, 150), 6),
                            }
                            if len(events + vehicle_events) % 3 == 0
                            else None
                        ),
                        "schema_version": "1",
                    }
                )
        vehicle_events.sort(key=lambda event: event["event_ts"])
        for seq, event in enumerate(vehicle_events, start=1):
            event["seq"] = seq
        if scenario in {"03-duplicate-same-seq", "04-duplicate-new-event-id"}:
            original = vehicle_events[1]
            duplicate = copy.deepcopy(original)
            duplicate["event_ts"] += 1
            duplicate["ingest_ts"] += 1
            if scenario == "04-duplicate-new-event-id":
                duplicate["event_id"] = event_id(rng)
            vehicle_events.append(duplicate)
        if scenario == "05-out-of-order":
            # Both CHARGING events arrive before COMPLETE, in reverse event time.
            vehicle_events[1]["ingest_ts"] = vehicle_events[2]["ingest_ts"] + 1
        if scenario == "06-late-after-close":
            late = copy.deepcopy(vehicle_events[1])
            late.update(
                event_id=event_id(rng),
                event_ts=vehicle_events[-1]["event_ts"] + MINUTE,
                ingest_ts=vehicle_events[-1]["ingest_ts"] + MINUTE,
                seq=len(vehicle_events) + 1,
            )
            vehicle_events.append(late)
        if scenario == "07-malformed":
            mutations = (
                ("event_id", None),
                ("session_id", None),
                ("charging_state", "UNKNOWN"),
                ("soc_pct", 101.0),
                ("power_kw", -1.0),
                ("charging_state", None),
            )
            latest = vehicle_events[-1]
            next_seq = len(vehicle_events) + 1
            for index, (field, value) in enumerate(mutations, start=1):
                malformed = copy.deepcopy(latest)
                malformed.update(
                    event_id=event_id(rng),
                    event_ts=latest["event_ts"] + index * 1_000,
                    ingest_ts=latest["ingest_ts"] + index * 1_000,
                    seq=next_seq + index,
                )
                if value is None:
                    del malformed[field]
                else:
                    malformed[field] = value
                vehicle_events.append(malformed)
        if scenario == "11-replay":
            first_session = vehicle_events[0]["session_id"]
            originals = [event for event in vehicle_events if event["session_id"] == first_session]
            for original in originals:
                replay = copy.deepcopy(original)
                replay["event_ts"] += 25 * MINUTE
                replay["ingest_ts"] += 25 * MINUTE
                vehicle_events.append(replay)
        events.extend(vehicle_events)

    # The final four lines are Heartbeats, even when vehicle clocks differ.
    final_ingest = max(event["ingest_ts"] for event in events) + 45 * MINUTE
    for index, vehicle_id in enumerate(vehicles):
        vehicle_events = [e for e in events if e["vehicle_id"] == vehicle_id]
        events.append(
            {
                "event_id": event_id(rng),
                "vehicle_id": vehicle_id,
                "session_id": None,
                "event_ts": max(e["event_ts"] for e in vehicle_events) + 45 * MINUTE,
                "ingest_ts": final_ingest + index,
                "seq": max(e["seq"] for e in vehicle_events) + 1,
                "charging_state": "HEARTBEAT",
                "soc_pct": None,
                "power_kw": None,
                "location": None,
                "schema_version": "1",
            }
        )
    return sorted(events, key=arrival_order)


def ota_fixture(scenario: str, rng: random.Random) -> list[dict]:
    """Cover canonical states, all aliases, clamping, and malformed inputs."""
    statuses = ["DOWNLOADING", "INSTALLING", "SUCCEEDED", "FAILED"]
    if scenario == "02-variant-status-strings":
        statuses = [
            "downloading",
            "DL",
            "download",
            "installing",
            "Inst",
            "done",
            "COMPLETE",
            "success",
            "failed",
            "Error",
        ]
    vehicles = [f"VH-{rng.getrandbits(32):08x}" for _ in range(4)]
    events = [
        {
            "event_id": event_id(rng),
            "vehicle_id": vehicles[index % len(vehicles)],
            "campaign_id": f"OTA-{index % 2:08x}",
            "status": status,
            "progress_pct": (
                [-5.0, 105.0, 50.0][index % 3]
                if scenario == "02-variant-status-strings"
                else 25.0 * index
            ),
            "event_ts": BASE_TS + index * MINUTE,
            "schema_version": "1",
        }
        for index, status in enumerate(statuses)
    ]
    if scenario == "03-malformed":
        events[0]["status"] = "UNKNOWN"
        del events[1]["event_id"]
        events[2]["progress_pct"] = "not-a-number"
        del events[3]["vehicle_id"]
    return events


def generate(output: Path, seed: int = DEFAULT_SEED) -> None:
    """Write only the fourteen fixture files, in stable JSONL encoding."""
    rng = random.Random(seed)
    for name, scenarios, build in (
        ("charging-session", CHARGING_SCENARIOS, charging_fixture),
        ("ota-status", OTA_SCENARIOS, ota_fixture),
    ):
        directory = output / name
        directory.mkdir(parents=True, exist_ok=True)
        for scenario in scenarios:
            records = build(scenario, rng)
            content = "".join(
                json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n"
                for record in records
            )
            (directory / f"{scenario}.jsonl").write_text(content, encoding="utf-8")


@click.command()
@click.option("--seed", default=DEFAULT_SEED, show_default=True, type=int)
@click.option("--output", default=ROOT / "fixtures", type=click.Path(path_type=Path))
def main(seed: int, output: Path) -> None:
    """Regenerate the synthetic charging-session and OTA-status fixtures."""
    generate(output, seed)


if __name__ == "__main__":
    main()
