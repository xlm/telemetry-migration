import json
import subprocess
from pathlib import Path

import click
import pytest
from click.testing import CliRunner

from harness import ci
from harness.ci import changed_paths, processors_to_gate

ALL = ["charging-session", "ota-status"]


@pytest.mark.parametrize(
    ("paths", "expected"),
    [
        ([], []),
        (["processors/ota-status/src/core.rs"], ["ota-status"]),
        (["manifests/charging-session.processor.yaml"], ["charging-session"]),
        (
            ["processors/ota-status/src/core.rs", "processors/charging-session/src/core.rs"],
            ALL,
        ),
        (["README.md", "docs/cutover.md", "AGENTS.md", ".agents/skills/x/SKILL.md"], []),
        (["evidence/ota-status/abc1234/evidence.md"], []),
        (["manifests/charging-session.job.yaml"], ALL),
        (["goldens/ota-status/01-normal.status.jsonl"], ALL),
        (["harness/src/harness/gate.py"], ALL),
        ([".github/workflows/gate.yml"], ALL),
        (["manifests/classification.yaml"], ALL),
        (["schemas/ota-event.v1.schema.json"], ALL),
        (["Makefile"], ALL),
        (["rust-toolchain.toml"], ALL),
        (["processors/telemetry-core/src/lib.rs"], ALL),
        (["processors/Cargo.toml"], ALL),
        (["processors/Cargo.lock"], ALL),
        (["processors/new-processor/src/main.rs"], ALL),
        (["processors/ota-status-v2/src/main.rs"], ALL),
        (["processors/ota-status"], ALL),
        (["manifests/ota-status.processor.yaml.bak"], []),
        (["manifests/nested/ota-status.processor.yaml"], []),
        (["docs/Makefile", "docs/processors/ota-status/x.md"], []),
        (["./processors/ota-status/src/core.rs"], ["ota-status"]),
    ],
)
def test_processors_to_gate(paths, expected):
    assert processors_to_gate(paths) == expected


def test_processors_to_gate_widens_to_all_after_a_single_owner():
    paths = ["processors/ota-status/src/core.rs", "contracts/ota-status.v1.schema.json"]
    assert processors_to_gate(paths) == ALL


def _git(root: Path, *arguments: str):
    return subprocess.run(
        ["git", "-c", "user.name=Test", "-c", "user.email=test@example.com", *arguments],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )


def _write(root: Path, path: str, text: str = "x\n"):
    target = root / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(text, encoding="utf-8")


def test_changed_paths_lists_branch_changes_since_merge_base(tmp_path):
    _git(tmp_path, "init", "-b", "main")
    _write(tmp_path, "fixtures/ota-status/a.jsonl")
    _git(tmp_path, "add", ".")
    _git(tmp_path, "commit", "-m", "baseline")
    _git(tmp_path, "checkout", "-b", "feature")
    (tmp_path / "processors" / "ota-status").mkdir(parents=True)
    _git(tmp_path, "mv", "fixtures/ota-status/a.jsonl", "processors/ota-status/a.jsonl")
    _git(tmp_path, "commit", "-m", "move")
    _git(tmp_path, "checkout", "main")
    _write(tmp_path, "jobs/later.scala")
    _git(tmp_path, "add", ".")
    _git(tmp_path, "commit", "-m", "main moves on")
    _git(tmp_path, "checkout", "feature")

    paths = changed_paths("main", root=tmp_path)

    assert sorted(paths) == ["fixtures/ota-status/a.jsonl", "processors/ota-status/a.jsonl"]
    assert processors_to_gate(paths) == ALL


def test_changed_paths_reports_unknown_base(tmp_path):
    _git(tmp_path, "init", "-b", "main")
    _write(tmp_path, "README")
    _git(tmp_path, "add", ".")
    _git(tmp_path, "commit", "-m", "baseline")

    with pytest.raises(click.ClickException, match="git diff against origin/main failed"):
        changed_paths("origin/main", root=tmp_path)


def test_main_prints_json_list(monkeypatch):
    monkeypatch.setattr(ci, "changed_paths", lambda base: ["processors/ota-status/src/core.rs"])

    result = CliRunner().invoke(ci.main, ["--base", "origin/main"])

    assert result.exit_code == 0
    assert json.loads(result.output) == ["ota-status"]
