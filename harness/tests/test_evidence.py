import json
import os
import shutil
from pathlib import Path

import pytest
import yaml

import harness.evidence as evidence_module
from harness.common import ROOT, StepResult
from harness.evidence import _markdown, write_evidence
from harness.privacy import compare_manifests

STEP_NAMES = ("Harness", "Contract", "Downstream System", "Privacy", "Path check")
IDENTITY_KEYS = {
    "processor",
    "sha",
    "short_sha",
    "worktree_dirty",
    "base",
    "baseline_manifest_version",
    "processor_manifest_version",
    "timestamp_utc",
    "reproduce_command",
    "commit_url",
    "branch",
    "pr_number",
}


@pytest.fixture(autouse=True)
def clear_github_environment(monkeypatch):
    for name in list(os.environ):
        if name.startswith("GITHUB_"):
            monkeypatch.delenv(name, raising=False)


def _fixture_counts(fixtures=3, records=14):
    counts = []
    for index in range(fixtures):
        golden = records // fixtures + (index < records % fixtures)
        counts.append(
            {
                "fixture": f"fixture-{index + 1:02}",
                "status": "green",
                "golden": golden,
                "candidate": golden,
                "identical": golden,
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
        )
    return {"fixtures": fixtures, "fixture_counts": counts}


def _step(name, status="green", findings=None, counts=None, summary=None):
    return StepResult(
        name,
        status,
        summary or f"{name} passed",
        counts or {},
        findings or [],
    )


def _validation(statuses=None, findings=None, counts=None, fixtures=3, records=14):
    statuses = statuses or {}
    findings = findings or {}
    counts = counts or {}
    steps = []
    for name in STEP_NAMES:
        default = "skipped" if name == "Downstream System" else "green"
        step_counts = counts.get(name)
        if name == "Harness" and step_counts is None:
            step_counts = _fixture_counts(fixtures, records)
        if name == "Privacy" and step_counts is None:
            step_counts = {"disposition": "no-material-change", "label": "no material change"}
        steps.append(
            _step(
                name,
                statuses.get(name, default),
                findings.get(name),
                step_counts,
            )
        )
    return steps


def _pack(tmp_path, steps, *, processor="ota-status", root=ROOT, base="origin/main", name="pack"):
    result = write_evidence(
        processor,
        base,
        steps,
        root=root,
        evidence_dir=tmp_path / name,
    )
    destination = Path(result.counts["path"])
    return (
        json.loads((destination / "evidence.json").read_text(encoding="utf-8")),
        (destination / "evidence.md").read_text(encoding="utf-8"),
    )


def test_green_ota_status_has_compact_executive_first_screen(tmp_path):
    evidence, markdown = _pack(tmp_path, _validation())

    assert evidence["summary"]["alert"] == "NOTE"
    assert evidence["summary"]["headline"] == (
        "Same data, same use as the approved Baseline. Output matches the legacy Job on all "
        "3 test scenarios (14 records)."
    )
    assert evidence["residual_uncertainties"][0] == (
        "Compared on 3 test scenarios (14 records) only. Behaviour that no scenario exercises is not compared."
    )
    assert "| 🟢 green | 🟢 green | ⚪ skipped | 🟢 green | 🟢 green |" in markdown
    assert "## What failed" not in markdown
    assert markdown.index("## Residual uncertainties") < markdown.index("<details>")
    assert markdown.index("## Identity") > markdown.index("<details>")
    for section in (
        "## Identity",
        "## Inventory",
        "### Harness per-fixture counts",
        "## Disposition",
    ):
        assert markdown.index(section) > markdown.index("<details>")
    first_screen = markdown.split("<details>", 1)[0]
    assert len([line for line in first_screen.splitlines() if line.strip()]) <= 15


def test_green_clarification_uses_important_alert_and_headline(tmp_path):
    finding = {
        "check": "runtime",
        "type": "review-clarification",
        "disposition": "clarification",
        "message": "reviewer confirmation needed",
    }
    evidence, _markdown_text = _pack(
        tmp_path,
        _validation(
            findings={"Privacy": [finding]},
        ),
    )

    assert evidence["summary"]["alert"] == "IMPORTANT"
    assert evidence["summary"]["headline"] == (
        "Output matches the legacy Job on all 3 test scenarios (14 records), "
        "with 1 clarification for the reviewer to confirm."
    )


def test_residual_uncertainty_uses_singular_counts(tmp_path):
    evidence, _markdown_text = _pack(
        tmp_path,
        _validation(fixtures=1, records=1),
        name="singular-coverage",
    )

    assert evidence["residual_uncertainties"][0] == (
        "Compared on 1 test scenario (1 record) only. Behaviour that no scenario exercises is not compared."
    )


def test_all_fixture_crashes_lead_red_headline_and_failures(tmp_path):
    crashes = [
        {"type": "non-zero-exit", "fixture": f"fixture-{index:02}", "exit_code": 101}
        for index in range(1, 12)
    ]
    drifts = [
        {
            "type": "drift",
            "fixture": f"fixture-{index:02}",
            "counts": {
                "candidate": 0,
                "dlq_candidate": 0,
                "missing": 1,
                "dlq_missing": 1,
            },
        }
        for index in range(1, 12)
    ]
    statuses = {"Harness": "red", "Downstream System": "red"}
    findings = {
        "Harness": crashes + drifts,
        "Downstream System": [
            {"type": "downstream-crash", "fixture": "fixture-01", "message": "script failed"}
        ],
    }
    evidence, markdown = _pack(
        tmp_path,
        _validation(
            statuses,
            findings,
            counts={"Harness": _fixture_counts(11, 92)},
            fixtures=11,
            records=92,
        ),
    )

    assert evidence["summary"]["alert"] == "CAUTION"
    assert evidence["summary"]["headline"] == (
        "Blocked at Harness: the Processor crashed on all 11 test scenarios, so none of its "
        "output could be compared with the legacy Job. Also red: Downstream System."
    )
    assert evidence["residual_uncertainties"][0] == (
        "Compared on 11 test scenarios (92 records) only. Behaviour that no scenario exercises is not compared."
    )
    assert evidence["summary"]["failures"][0]["sentence"] == (
        "the Processor crashed (exit 101) on 11 of 11 scenarios. "
        "11 records and 11 dead-letter entries that the legacy Job produces are missing."
    )
    assert evidence["summary"]["failures"][1]["sentence"] == (
        "the warranty script crashed on 1 scenario."
    )
    assert "- **Harness:**" in markdown
    assert "- **Downstream System:**" in markdown


def test_changed_value_drift_uses_specified_example(tmp_path):
    counts = _fixture_counts(11, 20)
    counts["fixture_counts"][0].update(
        {
            "candidate": 1,
            "identical": 0,
            "changed": 1,
        }
    )
    finding = {
        "type": "drift",
        "fixture": "CS-00000001",
        "counts": {"changed": 1, "candidate": 1, "dlq_candidate": 0},
        "changed": [
            {
                "key": "CS-00000001",
                "fields": [
                    {
                        "field": "energy_kwh",
                        "golden": 0.662,
                        "processor": 0.663,
                    }
                ],
            }
        ],
    }
    evidence, markdown = _pack(
        tmp_path,
        _validation(
            {"Harness": "red"},
            {"Harness": [finding]},
            counts={"Harness": counts},
            fixtures=11,
            records=20,
        ),
    )

    assert evidence["summary"]["headline"] == (
        "Blocked at Harness: output differs from the legacy Job in 1 of 11 test scenarios."
    )
    assert evidence["summary"]["failures"][0]["sentence"] == (
        "1 record differs from the legacy Job in 1 of 11 scenarios, e.g. "
        "`CS-00000001` `energy_kwh`: legacy 0.662, new 0.663."
    )
    assert (
        "- **Harness:** 1 record differs from the legacy Job in 1 of 11 scenarios, e.g. "
        "`CS-00000001` `energy_kwh`: legacy 0.662, new 0.663."
    ) in markdown


def test_runtime_d3_location_leads_privacy_without_declared_change(tmp_path):
    finding = {
        "check": "runtime",
        "type": "location-in-output",
        "disposition": "violation",
        "topic": "vehicle.charging.sessions",
        "field": "end_location",
        "fixture": "01-normal",
        "count": 1,
        "message": "field end_location, Baseline unchanged",
    }
    evidence, markdown = _pack(
        tmp_path,
        _validation(
            {"Privacy": "red"},
            {"Privacy": [finding]},
        ),
        processor="charging-session",
    )

    assert evidence["summary"]["lead_step"] == "Privacy"
    assert evidence["summary"]["headline"] == (
        "Blocked at Privacy: location appears in `end_location` on "
        "`vehicle.charging.sessions`, sent to warranty and service, which the Baseline "
        "does not allow."
    )
    assert evidence["summary"]["baseline_unchanged"] == (
        "No declared change. The Processor Manifest matches the Baseline, but the output "
        "does not (see What failed)."
    )
    assert "**Privacy - policy violation, must be fixed:**" in markdown
    assert "Warranty and service read this topic." in markdown


def test_log_only_privacy_names_logs_in_baseline_unchanged(tmp_path):
    finding = {
        "check": "runtime",
        "type": "location-in-log",
        "disposition": "violation",
        "count": 1,
        "example": "location",
    }
    evidence, _markdown_text = _pack(
        tmp_path,
        _validation({"Privacy": "red"}, {"Privacy": [finding]}),
    )

    assert evidence["summary"]["baseline_unchanged"] == (
        "No declared change. The Processor Manifest matches the Baseline, but its logs do not "
        "(see What failed)."
    )


def test_output_and_log_privacy_name_both_in_baseline_unchanged(tmp_path):
    findings = [
        {
            "check": "runtime",
            "type": "location-in-output",
            "disposition": "violation",
            "topic": "vehicle.charging.sessions",
            "field": "end_location",
            "fixture": "01-normal",
            "count": 1,
            "message": "field end_location, Baseline unchanged",
        },
        {
            "check": "runtime",
            "type": "location-in-log",
            "disposition": "violation",
            "count": 1,
            "example": "location",
        },
    ]
    evidence, _markdown_text = _pack(
        tmp_path,
        _validation({"Privacy": "red"}, {"Privacy": findings}),
        processor="charging-session",
    )

    assert evidence["summary"]["baseline_unchanged"] == (
        "No declared change. The Processor Manifest matches the Baseline, but its output and "
        "logs do not (see What failed)."
    )


def test_manifest_d3_location_appears_as_baseline_change(tmp_path):
    root = tmp_path / "repo"
    manifests = root / "manifests"
    manifests.mkdir(parents=True)
    for path in (
        ROOT / "manifests" / "charging-session.job.yaml",
        ROOT / "manifests" / "classification.yaml",
        ROOT / "manifests" / "charging-session.processor.yaml",
    ):
        shutil.copy2(path, manifests / path.name)
    processor_manifest = manifests / "charging-session.processor.yaml"
    manifest = yaml.safe_load(processor_manifest.read_text(encoding="utf-8"))
    topic = next(
        item for item in manifest["outputs"] if item["topic"] == "vehicle.charging.sessions"
    )
    topic["fields"].append(
        {"name": "location", "classification": "sensitive-location", "derived_from": ["location"]}
    )
    processor_manifest.write_text(yaml.safe_dump(manifest, sort_keys=False), encoding="utf-8")
    findings, _summary = compare_manifests("charging-session", root=root)

    evidence, markdown = _pack(
        tmp_path,
        _validation(
            {"Privacy": "red"},
            {"Privacy": findings},
        ),
        processor="charging-session",
        root=root,
        name="manifest-pack",
    )

    assert evidence["summary"]["baseline_changes"] == [
        {
            "type": "new-output-field",
            "disposition": "violation",
            "sentence": (
                "The Processor Manifest adds `location` (sensitive-location) to "
                "`vehicle.charging.sessions`, read by warranty and service."
            ),
        }
    ]
    assert (
        "- The Processor Manifest adds `location` (sensitive-location) to "
        "`vehicle.charging.sessions`, read by warranty and service. "
        "*(policy violation, must be fixed)*"
    ) in markdown
    assert evidence["summary"]["headline"] == (
        "Blocked at Privacy: the Processor Manifest adds `location` (sensitive-location) to "
        "`vehicle.charging.sessions`, read by warranty and service."
    )


def test_path_check_precedes_privacy_in_red_headline(tmp_path):
    evidence, _markdown_text = _pack(
        tmp_path,
        _validation(
            {"Path check": "red", "Privacy": "red"},
            {
                "Path check": [
                    {"type": "protected-path", "path": "harness/src/harness/evidence.py"}
                ],
                "Privacy": [
                    {
                        "check": "runtime",
                        "type": "location-in-log",
                        "disposition": "violation",
                        "count": 1,
                        "example": "location",
                    }
                ],
            },
        ),
    )

    assert evidence["summary"]["lead_step"] == "Path check"
    assert evidence["summary"]["headline"].startswith("Blocked at Path check:")
    assert evidence["summary"]["headline"].endswith("Also red: Privacy.")


@pytest.mark.parametrize("name", STEP_NAMES)
def test_unknown_finding_type_uses_fallback_for_each_step(tmp_path, name):
    finding = {
        "type": "future-finding",
        "message": "future detail",
        "check": "runtime",
        "disposition": "violation",
    }
    evidence, _markdown_text = _pack(
        tmp_path,
        _validation(
            {name: "red"},
            {name: [finding]},
        ),
        name=f"unknown-{name.replace(' ', '-')}",
    )

    failure = next(item for item in evidence["summary"]["failures"] if item["step"] == name)
    assert failure["sentence"] == "1 `future-finding` finding, e.g. future detail."


def test_identity_links_resolve_proxy_github_environment_and_pull_request(tmp_path, monkeypatch):
    git_values = {
        ("rev-parse", "HEAD"): "0123456789abcdef",
        ("rev-parse", "--short", "HEAD"): "0123456",
        ("status", "--porcelain", "--untracked-files=normal"): "",
        ("rev-parse", "--abbrev-ref", "HEAD"): "main",
    }
    remote = "https://git-manager.devin.ai/proxy/github.com/xlm/telemetry-migration"

    def fake_git_value(_root, args, default):
        return git_values.get(
            tuple(args), remote if args == ["remote", "get-url", "origin"] else default
        )

    monkeypatch.setattr(evidence_module, "_git_value", fake_git_value)
    for index, origin in enumerate(
        (
            "https://git-manager.devin.ai/proxy/github.com/xlm/telemetry-migration",
            "git@github.com:xlm/telemetry-migration.git",
        )
    ):
        remote = origin
        evidence, markdown = _pack(
            tmp_path,
            _validation(),
            base="HEAD",
            name=f"remote-{index}",
        )
        assert evidence["identity"]["commit_url"] == (
            "https://github.com/xlm/telemetry-migration/commit/0123456789abcdef"
        )
        assert "Path check base: this commit" in markdown

    monkeypatch.setenv("GITHUB_SERVER_URL", "https://github.example")
    monkeypatch.setenv("GITHUB_REPOSITORY", "team/repo.git")
    monkeypatch.setenv("GITHUB_HEAD_REF", "feature/headline")
    monkeypatch.setenv("GITHUB_EVENT_NAME", "pull_request")
    monkeypatch.setenv("GITHUB_REF", "refs/pull/12/merge")
    evidence, markdown = _pack(tmp_path, _validation(), name="github-env")
    assert evidence["identity"]["commit_url"] == (
        "https://github.example/team/repo/commit/0123456789abcdef"
    )
    assert evidence["identity"]["branch"] == "feature/headline"
    assert evidence["identity"]["pr_number"] == 12
    assert "PR [#12](https://github.example/team/repo/pull/12)" in markdown
    assert "Path check base: this commit" not in markdown

    monkeypatch.setenv("GITHUB_EVENT_NAME", "push")
    evidence, _markdown_text = _pack(tmp_path, _validation(), name="push-event")
    assert evidence["identity"]["pr_number"] is None

    for name in ("GITHUB_SERVER_URL", "GITHUB_REPOSITORY", "GITHUB_HEAD_REF", "GITHUB_REF"):
        monkeypatch.delenv(name, raising=False)
    remote = "https://example.com/team/repo"
    git_values[("rev-parse", "HEAD")] = "unknown"
    git_values[("rev-parse", "--abbrev-ref", "HEAD")] = "HEAD"
    evidence, _markdown_text = _pack(tmp_path, _validation(), name="unknown-identity")
    assert evidence["identity"]["commit_url"] is None
    assert evidence["identity"]["branch"] is None


def test_json_schema_and_markdown_are_a_pure_render(tmp_path):
    manifest_finding = {
        "check": "manifest",
        "type": "new-output-field",
        "topic": "ota.status",
        "field": "new_field",
        "processor": "public",
        "disposition": "new-processing",
        "message": "new output field new_field",
    }
    evidence, markdown = _pack(
        tmp_path,
        _validation(
            {"Harness": "red", "Privacy": "red"},
            {
                "Harness": [{"type": "missing-file", "message": "missing fixture"}],
                "Privacy": [manifest_finding],
            },
        ),
    )

    assert {
        "identity",
        "inventory",
        "change_assessment",
        "validation",
        "disposition",
        "residual_uncertainties",
        "summary",
    } <= evidence.keys()
    assert IDENTITY_KEYS <= evidence["identity"].keys()
    assert _markdown(evidence) == markdown
    summary = evidence["summary"]
    assert summary["headline"] in markdown
    assert all(failure["sentence"] in markdown for failure in summary["failures"])
    assert summary["baseline_changes"][0]["sentence"] in markdown


def test_worst_case_evidence_stays_within_markdown_limit(tmp_path):
    long_value = "x" * 120
    fixtures = 30
    harness_findings = [
        {
            "type": "drift",
            "fixture": f"fixture-{index:02}",
            "counts": {"changed": 1, "candidate": 1, "dlq_candidate": 0},
            "changed": [
                {
                    "key": f"key-{long_value}",
                    "fields": [
                        {
                            "field": f"field-{long_value}",
                            "golden": long_value,
                            "processor": long_value,
                        }
                    ],
                }
            ],
        }
        for index in range(1, 21)
    ] + [
        {"type": "non-zero-exit", "fixture": f"fixture-{index:02}", "exit_code": 101}
        for index in range(1, 21)
    ]
    contract_findings = [
        {
            "type": "schema" if index % 2 else "semantic",
            "fixture": f"fixture-{index:02}",
            "key": f"key-{index}",
            "field": f"field-{long_value}",
            "message": long_value,
        }
        for index in range(1, 41)
    ]
    downstream_findings = [
        {
            "type": "flag-drift",
            "fixture": f"fixture-{index:02}",
            "changed": [
                {
                    "session_id": f"session-{long_value}",
                    "golden": long_value,
                    "processor": long_value,
                }
            ],
        }
        for index in range(1, 21)
    ] + [{"type": "downstream-crash", "fixture": f"fixture-{index:02}"} for index in range(1, 11)]
    privacy_findings = [
        {
            "check": "runtime",
            "type": ("location-in-log", "raw-vehicle-id-in-log", "unknown-privacy")[index % 3],
            "disposition": ("violation", "new-processing", "clarification")[index % 3],
            "count": 1,
            "example": long_value,
            "message": long_value,
        }
        for index in range(30)
    ]
    path_findings = [
        {"type": "protected-path", "path": f"protected-{long_value}-{index}"} for index in range(25)
    ] + [{"type": "git-failed", "message": long_value} for _ in range(5)]
    evidence, markdown = _pack(
        tmp_path,
        _validation(
            dict.fromkeys(STEP_NAMES, "red"),
            {
                "Harness": harness_findings,
                "Contract": contract_findings,
                "Downstream System": downstream_findings,
                "Privacy": privacy_findings,
                "Path check": path_findings,
            },
            counts={"Harness": _fixture_counts(fixtures, 300)},
            fixtures=fixtures,
            records=300,
        ),
        processor="charging-session",
    )

    assert all(len(step["findings"]) >= 20 for step in evidence["validation"])
    assert len(markdown) <= 20_000
    first_screen = markdown.split("<details>", 1)[0]
    first_path = f"protected-{long_value}-0"
    assert first_path not in first_screen
    assert f"`{first_path[:59]}…`" in first_screen
