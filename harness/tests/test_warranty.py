import importlib.util
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "downstream" / "warranty.py"
spec = importlib.util.spec_from_file_location("warranty", SCRIPT)
assert spec and spec.loader
warranty = importlib.util.module_from_spec(spec)
spec.loader.exec_module(warranty)
flag = warranty.flag


def test_warranty_flags_cover_energy_timeout_and_ok():
    assert (
        flag(
            {
                "energy_kwh": 10,
                "start_soc_pct": 10,
                "end_soc_pct": 12,
                "close_reason": "COMPLETE",
            }
        )
        == "HIGH_ENERGY_PER_SOC"
    )
    assert (
        flag(
            {
                "energy_kwh": 0,
                "start_soc_pct": 10,
                "end_soc_pct": 20,
                "close_reason": "TIMEOUT",
            }
        )
        == "TIMEOUT"
    )
    assert (
        flag(
            {
                "energy_kwh": 0,
                "start_soc_pct": 10,
                "end_soc_pct": 20,
                "close_reason": "COMPLETE",
            }
        )
        == "OK"
    )


def test_high_energy_precedes_timeout_and_null_soc_uses_delta_zero():
    assert (
        flag(
            {
                "energy_kwh": 2,
                "start_soc_pct": 10,
                "end_soc_pct": 11,
                "close_reason": "TIMEOUT",
            }
        )
        == "HIGH_ENERGY_PER_SOC"
    )
    assert (
        flag(
            {
                "energy_kwh": 2,
                "start_soc_pct": None,
                "end_soc_pct": 90,
                "close_reason": "COMPLETE",
            }
        )
        == "HIGH_ENERGY_PER_SOC"
    )
    assert (
        flag(
            {
                "energy_kwh": 1,
                "start_soc_pct": 10,
                "end_soc_pct": None,
                "close_reason": "COMPLETE",
            }
        )
        == "OK"
    )


def test_warranty_cli_prints_compact_flags_in_input_order(tmp_path):
    input_path = tmp_path / "sessions.jsonl"
    input_path.write_text(
        "\n".join(
            json.dumps(item)
            for item in (
                {
                    "session_id": "CS-1",
                    "energy_kwh": 0,
                    "start_soc_pct": 1,
                    "end_soc_pct": 2,
                    "close_reason": "TIMEOUT",
                },
                {
                    "session_id": "CS-2",
                    "energy_kwh": 10,
                    "start_soc_pct": 1,
                    "end_soc_pct": 2,
                    "close_reason": "COMPLETE",
                },
            )
        )
        + "\n",
        encoding="utf-8",
    )
    result = subprocess.run(
        [sys.executable, str(SCRIPT), str(input_path)],
        check=True,
        capture_output=True,
        text=True,
    )
    assert result.stdout.splitlines() == [
        '{"flag":"TIMEOUT","session_id":"CS-1"}',
        '{"flag":"HIGH_ENERGY_PER_SOC","session_id":"CS-2"}',
    ]
