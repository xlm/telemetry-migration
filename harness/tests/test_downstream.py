import json
from pathlib import Path

import yaml

from harness.downstream import run_downstream

ROOT = Path(__file__).resolve().parents[2]
WARRANTY_SCRIPT = ROOT / "downstream" / "warranty.py"


def _setup(root: Path, candidate_close_reason: str):
    (root / "manifests").mkdir(parents=True)
    (root / "fixtures" / "charging-session").mkdir(parents=True)
    (root / "goldens" / "charging-session").mkdir(parents=True)
    (root / "work").mkdir()
    (root / "downstream").mkdir()
    (root / "downstream" / "warranty.py").write_text(
        WARRANTY_SCRIPT.read_text(encoding="utf-8"), encoding="utf-8"
    )
    manifest = {
        "outputs": [
            {
                "topic": "vehicle.charging.sessions",
                "downstream": ["warranty"],
            },
            {
                "topic": "vehicle.charging.sessions.dlq",
                "downstream": ["warranty"],
            },
        ]
    }
    (root / "manifests" / "charging-session.job.yaml").write_text(
        yaml.safe_dump(manifest), encoding="utf-8"
    )
    summary = {
        "session_id": "CS-1",
        "energy_kwh": 0,
        "start_soc_pct": 10,
        "end_soc_pct": 20,
        "close_reason": "TIMEOUT",
    }
    fixture = root / "fixtures" / "charging-session" / "fixture.jsonl"
    fixture.write_text("{}\n", encoding="utf-8")
    golden = root / "goldens" / "charging-session" / "fixture.sessions.jsonl"
    golden.write_text(json.dumps(summary) + "\n", encoding="utf-8")
    candidate = summary | {"close_reason": candidate_close_reason}
    output = root / "work" / "fixture.out.jsonl"
    output.write_text(json.dumps(candidate) + "\n", encoding="utf-8")
    return root / "goldens", root / "work"


def test_downstream_compares_warranty_flags_against_golden(tmp_path):
    goldens_dir, work_dir = _setup(tmp_path, "TIMEOUT")
    result = run_downstream(
        "charging-session",
        root=tmp_path,
        goldens_dir=goldens_dir,
        work_dir=work_dir,
    )
    assert result.status == "green"
    assert result.summary == "1 sessions, 1 identical flags"


def test_downstream_reports_flag_drift(tmp_path):
    goldens_dir, work_dir = _setup(tmp_path, "COMPLETE")
    result = run_downstream(
        "charging-session",
        root=tmp_path,
        goldens_dir=goldens_dir,
        work_dir=work_dir,
    )
    assert result.status == "red"
    assert result.findings[0]["type"] == "flag-drift"
