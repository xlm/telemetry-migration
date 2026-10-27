import json
import re
import subprocess
from pathlib import Path

from harness.common import ROOT, StepResult
from harness.evidence import write_evidence
from harness.gate import PROTECTED_PATHS, check_paths, path_is_protected, print_report, run_gate

FAKE_PROCESSOR = ROOT / "harness" / "tests" / "fake_processor.py"


def test_protected_path_matching_includes_exact_rules_and_near_misses():
    cases = {
        "goldens/ota-status/a.jsonl": True,
        "fixtures/ota-status/a.jsonl": True,
        "harness2/file.py": False,
        "goldens.md": False,
        "manifests/charging-session.job.yaml": True,
        "manifests/nested/charging-session.job.yaml": False,
        "manifests/charging-session.processor.yaml": False,
        "processors/ota-status/main.rs": False,
        "contracts/schema.json": True,
        "jobs/example.scala": True,
        "downstream/warranty.py": True,
    }
    for path, expected in cases.items():
        assert bool(path_is_protected(path)) is expected


def test_evidence_reproduction_command_includes_bin_only_when_set(tmp_path):
    with_bin = write_evidence(
        "ota-status",
        "origin/main",
        [],
        root=ROOT,
        evidence_dir=tmp_path / "with-bin",
        bin_path=FAKE_PROCESSOR,
    )
    with_bin_evidence = Path(with_bin.counts["path"]) / "evidence.json"
    with_bin_command = json.loads(with_bin_evidence.read_text(encoding="utf-8"))["identity"][
        "reproduce_command"
    ]
    assert with_bin_command == "make gate PROCESSOR=ota-status BIN=harness/tests/fake_processor.py"

    external_bin = Path("/tmp/my dir/p")
    with_external_bin = write_evidence(
        "ota-status",
        "origin/main",
        [],
        root=ROOT,
        evidence_dir=tmp_path / "external-bin",
        bin_path=external_bin,
    )
    external_evidence = Path(with_external_bin.counts["path"]) / "evidence.json"
    external_command = json.loads(external_evidence.read_text(encoding="utf-8"))["identity"][
        "reproduce_command"
    ]
    assert external_command == "make gate PROCESSOR=ota-status BIN='/tmp/my dir/p'"

    without_bin = write_evidence(
        "ota-status",
        "origin/main",
        [],
        root=ROOT,
        evidence_dir=tmp_path / "without-bin",
    )
    without_bin_evidence = Path(without_bin.counts["path"]) / "evidence.json"
    without_bin_command = json.loads(without_bin_evidence.read_text(encoding="utf-8"))["identity"][
        "reproduce_command"
    ]
    assert without_bin_command == "make gate PROCESSOR=ota-status"

    with_space_base = write_evidence(
        "ota-status",
        "feature branch",
        [],
        root=ROOT,
        evidence_dir=tmp_path / "space-base",
    )
    with_space_base_evidence = Path(with_space_base.counts["path"]) / "evidence.json"
    with_space_base_command = json.loads(with_space_base_evidence.read_text(encoding="utf-8"))[
        "identity"
    ]["reproduce_command"]
    assert with_space_base_command == "make gate PROCESSOR=ota-status BASE='feature branch'"


def test_report_flattens_multiline_findings(capsys, tmp_path):
    finding = {
        "type": "downstream-crash",
        "fixture": "01-normal",
        "side": "golden",
        "message": "Traceback (most recent call last):\nFileNotFoundError: missing",
    }
    step = StepResult("Downstream System", "red", "0 sessions, 0 identical flags", {}, [finding])
    print_report("charging-session", "origin/main", [step], root=tmp_path)
    output = capsys.readouterr().out
    assert "Traceback" not in output
    assert "downstream-crash [01-normal/golden]: FileNotFoundError: missing" in output


def test_report_uses_generic_record_label_and_prints_evidence_path_once(capsys, tmp_path):
    evidence_path = "evidence/ota-status/abc1234/evidence.md"
    harness_step = StepResult(
        "Harness",
        "green",
        "3 fixtures, 14 identical records, 0 with Drift",
        {
            "fixture_counts": [
                {
                    "fixture": "01-normal",
                    "status": "green",
                    "golden": 4,
                    "candidate": 4,
                    "identical": 4,
                    "missing": 0,
                    "extra": 0,
                    "duplicate": 0,
                    "changed": 0,
                    "dlq_golden": 0,
                    "dlq_candidate": 0,
                    "dlq_missing": 0,
                    "dlq_extra": 0,
                    "exit_code": 0,
                }
            ]
        },
        [],
    )
    evidence_step = StepResult("Evidence Pack", "green", evidence_path, {}, [])

    print_report(
        "ota-status",
        "HEAD",
        [harness_step, evidence_step],
        root=tmp_path,
    )

    output = capsys.readouterr().out
    assert "records golden=4 candidate=4" in output
    assert "sessions golden=" not in output
    assert output.count(evidence_path) == 1


def test_protected_path_rules_match_agent_and_build_plan_lists():
    agents = ROOT / "AGENTS.md"
    if not agents.exists():
        agents = ROOT / "docs" / "phase2" / "AGENTS.md"
    agent_text = agents.read_text(encoding="utf-8")
    read_only = agent_text.split("## Gate inputs are read-only", 1)[1]
    read_only = read_only.split("\n## ", 1)[0]
    agent_rules = set(re.findall(r"- `([^`]+)`", read_only))
    assert set(PROTECTED_PATHS) == agent_rules

    plan = ROOT / "docs" / "build-plan.md"
    if plan.exists():
        text = plan.read_text(encoding="utf-8")
        section = re.search(r"5\.\s+\*\*Path check\.\*\*(.*?)(?=\n6\.)", text, re.DOTALL)
        assert section
        list_text = section.group(1).split("must not touch", 1)[1].split(" Base is ", 1)[0]
        plan_rules = set(re.findall(r"`([^`]+)`", list_text))
        assert set(PROTECTED_PATHS) == plan_rules


def _git(root: Path, *arguments: str):
    return subprocess.run(
        ["git", *arguments],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )


def _git_repo(root: Path):
    _git(root, "init", "-b", "main")
    (root / "README").write_text("baseline\n", encoding="utf-8")
    _git(root, "add", "README")
    _git(
        root,
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "commit",
        "-m",
        "baseline",
    )
    _git(root, "checkout", "-b", "feature")


def _commit(root: Path, message: str, *paths: str):
    _git(root, "add", *paths)
    _git(
        root,
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "commit",
        "-m",
        message,
    )


def test_path_check_detects_committed_and_untracked_protected_paths(tmp_path):
    root = tmp_path / "committed"
    root.mkdir()
    _git_repo(root)
    protected = root / "fixtures" / "new.jsonl"
    protected.parent.mkdir()
    protected.write_text("{}\n", encoding="utf-8")
    _commit(root, "add protected fixture", "fixtures/new.jsonl")
    committed_result = check_paths("main", root=root)
    assert committed_result.status == "red"
    assert committed_result.findings[0]["path"] == "fixtures/new.jsonl"

    other = tmp_path / "untracked"
    other.mkdir()
    _git_repo(other)
    untracked = other / "harness" / "new.py"
    untracked.parent.mkdir()
    untracked.write_text("pass\n", encoding="utf-8")
    untracked_result = check_paths("main", root=other)
    assert untracked_result.status == "red"
    assert untracked_result.findings[0]["path"] == "harness/new.py"


def test_path_check_allows_processors_only_changes_and_reports_bad_base(tmp_path):
    root = tmp_path / "repo"
    root.mkdir()
    _git_repo(root)
    processor = root / "processors" / "main.rs"
    processor.parent.mkdir()
    processor.write_text("fn main() {}\n", encoding="utf-8")
    _commit(root, "add processor", "processors/main.rs")
    assert check_paths("main", root=root).status == "green"

    bad_base = check_paths("does-not-exist", root=root)
    assert bad_base.status == "red"
    assert bad_base.findings[0]["type"] == "git-failed"


def _generate_goldens(goldens_dir: Path):
    fixture_dir = ROOT / "fixtures" / "ota-status"
    output_dir = goldens_dir / "ota-status"
    output_dir.mkdir(parents=True)
    for fixture in sorted(fixture_dir.glob("*.jsonl")):
        output = output_dir / f"{fixture.stem}.status.jsonl"
        dlq = output_dir / f"{fixture.stem}.dlq.jsonl"
        log = output_dir / f"{fixture.stem}.log.jsonl"
        subprocess.run(
            [
                str(FAKE_PROCESSOR),
                "--input",
                str(fixture),
                "--output",
                str(output),
                "--dlq",
                str(dlq),
                "--log",
                str(log),
            ],
            check=True,
        )
        log.unlink()


def test_gate_runs_all_six_steps_and_writes_evidence_after_harness_drift(tmp_path):
    goldens_dir = tmp_path / "goldens"
    _generate_goldens(goldens_dir)
    work_dir = tmp_path / "work"
    evidence_dir = tmp_path / "evidence"
    steps, _ = run_gate(
        "ota-status",
        base="HEAD",
        bin_path=FAKE_PROCESSOR,
        root=ROOT,
        goldens_dir=goldens_dir,
        work_dir=work_dir,
        evidence_dir=evidence_dir,
    )
    assert [step.name for step in steps] == [
        "Harness",
        "Contract",
        "Downstream System",
        "Privacy",
        "Path check",
        "Evidence Pack",
    ]
    assert steps[0].status == "green"
    assert steps[1].status == "green"
    assert steps[0].summary == "3 fixtures, 14 identical records, 0 with Drift"
    assert steps[2].status == "skipped"
    assert steps[3].status == "green"
    assert steps[2].summary == (
        "no Downstream System script for this Processor's output (downstream: ota)"
    )
    evidence_path = Path(steps[-1].counts["path"])
    assert (evidence_path / "evidence.json").is_file()
    markdown = evidence_path / "evidence.md"
    assert markdown.is_file()
    markdown_lines = markdown.read_text(encoding="utf-8").splitlines()
    assert len(markdown_lines) < 150
    evidence = json.loads((evidence_path / "evidence.json").read_text(encoding="utf-8"))
    assert evidence["identity"]["reproduce_command"] == (
        "make gate PROCESSOR=ota-status BASE=HEAD BIN=harness/tests/fake_processor.py"
    )
    assert all(
        index == 0 or markdown_lines[index - 1] == ""
        for index, line in enumerate(markdown_lines)
        if line.startswith("#")
    )

    golden = goldens_dir / "ota-status" / "01-normal.status.jsonl"
    contents = golden.read_text(encoding="utf-8")
    golden.write_text(contents.replace("DOWNLOADING", "FAILED", 1), encoding="utf-8")
    drift_steps, drift_exit = run_gate(
        "ota-status",
        base="HEAD",
        bin_path=FAKE_PROCESSOR,
        root=ROOT,
        goldens_dir=goldens_dir,
        work_dir=work_dir,
        evidence_dir=evidence_dir,
    )
    assert drift_exit == 1
    assert len(drift_steps) == 6
    assert drift_steps[0].status == "red"
    assert drift_steps[-1].name == "Evidence Pack"
    assert drift_steps[-1].status == "green"
