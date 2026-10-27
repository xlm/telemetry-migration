import os

from harness import run_processor
from harness.run_processor import run_harness


def _fixture_tree(tmp_path):
    fixture_dir = tmp_path / "fixtures" / "ota-status"
    fixture_dir.mkdir(parents=True)
    (fixture_dir / "01-sample.jsonl").write_text('{"event_id":"event-1"}\n', encoding="utf-8")

    cargo_dir = tmp_path / "bin"
    cargo_dir.mkdir()
    cargo = cargo_dir / "cargo"
    cargo_marker = tmp_path / "cargo-ran"
    cargo.write_text(
        f"#!/bin/sh\nprintf called > '{cargo_marker}'\nexit 17\n",
        encoding="utf-8",
    )
    os.chmod(cargo, 0o755)

    binary = tmp_path / "processors" / "target" / "release" / "ota-status"
    binary.parent.mkdir(parents=True)
    marker = tmp_path / "binary-ran"
    binary.write_text(f"#!/bin/sh\nprintf called > '{marker}'\n", encoding="utf-8")
    os.chmod(binary, 0o755)
    return cargo_dir, cargo_marker, marker, binary


def _assert_not_run(result, marker, work_dir):
    assert not marker.exists()
    assert result.status == "red"
    assert any(finding["type"] == "build-failed" for finding in result.findings)
    fixture = result.counts["fixture_counts"][0]
    assert fixture["status"] == "red"
    assert fixture["findings"] == [
        {
            "type": "not-run",
            "fixture": "01-sample",
            "message": "Processor not run: build failed",
        }
    ]
    for count in (
        "golden",
        "candidate",
        "identical",
        "missing",
        "extra",
        "changed",
        "duplicate",
        "dlq_golden",
        "dlq_candidate",
        "dlq_missing",
        "dlq_extra",
    ):
        assert fixture[count] == 0
    assert fixture["exit_code"] is None
    assert not (work_dir / "01-sample.out.jsonl").exists()
    assert not (work_dir / "01-sample.dlq.jsonl").exists()
    assert not (work_dir / "01-sample.log.jsonl").exists()
    assert (work_dir / "01-sample.stdout.log").read_text(encoding="utf-8") == ""
    assert (work_dir / "01-sample.stderr.log").read_text(encoding="utf-8") == ""


def test_failed_cargo_build_does_not_run_preexisting_binary(tmp_path, monkeypatch):
    cargo_dir, cargo_marker, marker, _ = _fixture_tree(tmp_path)
    monkeypatch.setenv("PATH", f"{cargo_dir}{os.pathsep}{os.environ.get('PATH', '')}")
    work_dir = tmp_path / "work"

    result = run_harness("ota-status", root=tmp_path, work_dir=work_dir)

    _assert_not_run(result, marker, work_dir)
    assert cargo_marker.exists()


def test_cargo_oserror_does_not_run_preexisting_binary(tmp_path, monkeypatch):
    _, _, marker, _ = _fixture_tree(tmp_path)
    calls = []

    def fail_cargo(arguments, **kwargs):
        calls.append(arguments)
        raise OSError("cargo is unavailable")

    monkeypatch.setattr(run_processor.subprocess, "run", fail_cargo)
    work_dir = tmp_path / "work"

    result = run_harness("ota-status", root=tmp_path, work_dir=work_dir)

    assert len(calls) == 1
    assert calls[0][0] == "cargo"
    _assert_not_run(result, marker, work_dir)
