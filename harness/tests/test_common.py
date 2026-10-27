from harness.common import PROCESSORS, StepResult, read_jsonl


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
