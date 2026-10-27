import hashlib
import json
import subprocess
from pathlib import Path

FAKE_PROCESSOR = Path(__file__).with_name("fake_processor.py")


def _event(event_id, status, progress=40):
    return {
        "event_id": event_id,
        "vehicle_id": "VH-1",
        "campaign_id": "campaign",
        "status": status,
        "progress_pct": progress,
        "event_ts": 1,
        "schema_version": "invalid-but-ignored",
    }


def test_fake_processor_normalizes_statuses_and_dead_letters_invalid_lines(tmp_path):
    input_path = tmp_path / "input.jsonl"
    input_path.write_text(
        "\n".join(
            [
                json.dumps(_event("one", "succeeded", 101)),
                "{unparseable",
                json.dumps(_event(42, "unknown")),
                json.dumps(_event("two", " SUCCEEDED ")),
            ]
        )
        + "\n",
        encoding="utf-8",
    )
    output_path = tmp_path / "output.jsonl"
    dlq_path = tmp_path / "dlq.jsonl"
    log_path = tmp_path / "log.jsonl"

    subprocess.run(
        [
            str(FAKE_PROCESSOR),
            "--input",
            str(input_path),
            "--output",
            str(output_path),
            "--dlq",
            str(dlq_path),
            "--log",
            str(log_path),
        ],
        check=True,
    )

    output = [json.loads(line) for line in output_path.read_text().splitlines()]
    assert output == [
        {
            "campaign_id": "campaign",
            "event_id": "one",
            "event_ts": 1,
            "progress_pct": 100.0,
            "schema_version": "1",
            "status": "SUCCEEDED",
            "vehicle_id": "VH-1",
        }
    ]
    dlq = [json.loads(line) for line in dlq_path.read_text().splitlines()]
    assert dlq == [
        {"event_id": None, "reason": "MISSING_FIELD", "vehicle_id": None},
        {"event_id": None, "reason": "MISSING_FIELD", "vehicle_id": "VH-1"},
        {"event_id": "two", "reason": "UNKNOWN_STATUS", "vehicle_id": "VH-1"},
    ]
    logs = [json.loads(line) for line in log_path.read_text().splitlines()]
    assert logs[0]["fields"]["vehicle_id_hash"] == hashlib.sha256(b"VH-1").hexdigest()[:12]
    assert logs[1]["fields"]["vehicle_id_hash"] == ""
    assert logs[1]["fields"]["reason"] == "MISSING_FIELD"
