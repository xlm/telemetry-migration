import pytest

from harness.common import PROCESSORS, StepResult, parse_jsonl, read_jsonl


def test_read_jsonl_returns_records_and_parse_errors(tmp_path):
    path = tmp_path / "events.jsonl"
    path.write_text('{"event_id":"1"}\nnot-json\n[1]\n', encoding="utf-8")
    records, errors = read_jsonl(path)
    assert records == [{"event_id": "1"}]
    assert len(errors) == 2
    assert "line 2" in errors[0]
    assert "line 3" in errors[1]


def test_read_jsonl_reports_missing_file(tmp_path):
    records, errors = read_jsonl(tmp_path / "missing.jsonl")
    assert records == []
    assert "missing or unreadable file" in errors[0]


def test_registry_and_step_result_serialization():
    assert PROCESSORS["charging-session"].key == "session_id"
    assert PROCESSORS["ota-status"].key == "event_id"
    result = StepResult("Privacy", "green", "no findings", {}, [])
    assert result.to_dict() == {
        "name": "Privacy",
        "status": "green",
        "summary": "no findings",
        "counts": {},
        "findings": [],
    }


@pytest.mark.parametrize(
    "line",
    [
        '{"energy_kwh": NaN}',
        '{"energy_kwh": Infinity}',
        '{"energy_kwh": -Infinity}',
        '{"nested": {"values": [1, NaN]}}',
        '{"energy_kwh": 1.2, "energy_kwh": 999}',
        '{"nested": {"a": 1, "a": 2}}',
    ],
)
def test_parse_jsonl_rejects_non_standard_numbers_and_duplicate_keys(line):
    records, errors = parse_jsonl('{"ok": 1}\n' + line + "\n")
    assert records == [{"ok": 1}]
    assert len(errors) == 1
    assert errors[0].startswith("line 2: invalid JSON")


def test_parse_jsonl_rejects_overflowing_float_values():
    records, errors = parse_jsonl('{"x": 1e999}\n{"x": -1e999}\n{"x": 1.5e3}\n')
    assert records == [{"x": 1500.0}]
    assert len(errors) == 2
    assert errors[0].startswith("line 1: invalid JSON")
    assert errors[1].startswith("line 2: invalid JSON")
    assert "finite" in errors[0]
    assert "finite" in errors[1]


@pytest.mark.parametrize("separator", ["\u2028", "\u2029", "\x85"])
def test_parse_jsonl_splits_records_on_newline_only(separator):
    records, errors = parse_jsonl(f'{{"note": "a{separator}b"}}\n{{"ok": 1}}\n')
    assert errors == []
    assert records == [{"note": f"a{separator}b"}, {"ok": 1}]


def test_parse_jsonl_accepts_crlf_and_quoted_constants():
    records, errors = parse_jsonl('{"status": "NaN"}\r\n{"status": "Infinity"}\r\n')
    assert errors == []
    assert records == [{"status": "NaN"}, {"status": "Infinity"}]
