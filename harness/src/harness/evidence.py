"""Write the structured JSON and readable Markdown Evidence Pack."""

from __future__ import annotations

import json
import subprocess
from datetime import UTC, datetime
from pathlib import Path

import yaml

from harness.common import ROOT, StepResult
from harness.privacy import DISPLAY_LABELS

RESIDUAL_UNCERTAINTIES = [
    "Fixture coverage: equivalence is shown only on the committed fixtures. Rules or inputs no fixture exercises are unverified.",
    "The runtime scan is value-based, not flow-based. It catches fixture values copied into outputs or logs, not transformed or derived leakage.",
    "No broker semantics are tested. Ordering across partitions, redelivery, consumer group offsets, and idempotent production are outside the Harness.",
]


def write_evidence(
    processor: str,
    base: str,
    validation_steps: list[StepResult],
    *,
    root: Path = ROOT,
    evidence_dir: Path | None = None,
) -> StepResult:
    evidence_dir = evidence_dir or root / "evidence"
    sha = _git_value(root, ["rev-parse", "HEAD"], "unknown")
    short_sha = _git_value(root, ["rev-parse", "--short", "HEAD"], "unknown")
    dirty = bool(_git_value(root, ["status", "--porcelain", "--untracked-files=normal"], ""))
    baseline = _load_yaml(root / "manifests" / f"{processor}.job.yaml")
    manifest = _load_yaml(root / "manifests" / f"{processor}.processor.yaml")
    privacy_step = next((step for step in validation_steps if step.name == "Privacy"), None)
    privacy_counts = privacy_step.counts if privacy_step else {}
    privacy_findings = (
        [finding for finding in privacy_step.findings if finding.get("check") == "manifest"]
        if privacy_step
        else []
    )
    red_steps = [step.name for step in validation_steps if step.status == "red"]
    reproduce = f"make gate PROCESSOR={processor}"
    if base != "origin/main":
        reproduce += f" BASE={base}"
    inventory = _inventory(manifest)
    evidence = {
        "identity": {
            "processor": processor,
            "sha": sha,
            "short_sha": short_sha,
            "worktree_dirty": dirty,
            "base": base,
            "baseline_manifest_version": baseline.get("version"),
            "processor_manifest_version": manifest.get("version"),
            "timestamp_utc": datetime.now(UTC).isoformat().replace("+00:00", "Z"),
            "reproduce_command": reproduce,
        },
        "inventory": inventory,
        "change_assessment": privacy_findings,
        "validation": [step.to_dict() for step in validation_steps[:5]],
        "disposition": {
            "privacy": privacy_counts.get("disposition", "no-material-change"),
            "privacy_label": privacy_counts.get("label", DISPLAY_LABELS["no-material-change"]),
            "gate_verdict": "red" if red_steps else "green",
            "red_steps": red_steps,
        },
        "residual_uncertainties": RESIDUAL_UNCERTAINTIES,
    }
    destination = evidence_dir / processor / short_sha
    try:
        destination.mkdir(parents=True, exist_ok=True)
        (destination / "evidence.json").write_text(
            json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        markdown = _markdown(processor, short_sha, evidence)
        (destination / "evidence.md").write_text(markdown, encoding="utf-8")
    except OSError as error:
        return StepResult(
            "Evidence Pack",
            "red",
            f"could not write Evidence Pack: {error}",
            {"path": str(destination)},
            [{"type": "write-failed", "message": str(error), "path": str(destination)}],
        )
    return StepResult(
        "Evidence Pack",
        "green",
        f"{destination.relative_to(root) if destination.is_relative_to(root) else destination}/evidence.md",
        {"path": str(destination)},
        [],
    )


def _markdown(processor: str, short_sha: str, evidence: dict) -> str:
    identity = evidence["identity"]
    inventory = evidence["inventory"]
    steps = evidence["validation"]
    lines = [
        f"# Evidence Pack: {processor} @ {short_sha}",
        "",
        "## Identity",
        "",
        f"- Processor: `{processor}`",
        f"- Commit SHA: `{identity['sha']}`",
        f"- Worktree dirty: `{str(identity['worktree_dirty']).lower()}`",
        f"- Base: `{identity['base']}`",
        f"- Manifest versions: Baseline `{identity['baseline_manifest_version']}`, Processor `{identity['processor_manifest_version']}`",
        f"- Timestamp (UTC): `{identity['timestamp_utc']}`",
        f"- Reproduce: `{identity['reproduce_command']}`",
        "",
        "## Inventory",
        "",
        "### Inputs",
        "",
        "| Topic | Field | Classification | Purpose |",
        "| --- | --- | --- | --- |",
    ]
    for topic in inventory["inputs"]:
        for field in topic["fields"]:
            lines.append(
                f"| {topic['topic']} | {field['name']} | {field.get('classification', '')} | {field.get('purpose', '')} |"
            )
    lines.extend(
        [
            "",
            "### Outputs",
            "",
            "| Topic | Downstream System | Field | Classification | Derived from |",
            "| --- | --- | --- | --- | --- |",
        ]
    )
    for topic in inventory["outputs"]:
        for field in topic["fields"]:
            lines.append(
                f"| {topic['topic']} | {', '.join(topic['downstream'])} | {field['name']} | {field.get('classification', '')} | {', '.join(field.get('derived_from', []))} |"
            )
    lines.extend(
        [
            "",
            f"- Retention: `{inventory['retention_days']}` days",
            f"- Logging restrictions: forbidden `{', '.join(inventory['logging']['forbidden'])}`, hashed `{', '.join(inventory['logging']['hashed'])}`",
            "",
            "## Change assessment",
            "",
        ]
    )
    _append_findings(lines, evidence["change_assessment"])
    lines.extend(
        [
            "",
            "## Validation",
            "",
            "| Step | Status | Summary |",
            "| --- | --- | --- |",
        ]
    )
    for step in steps:
        lines.append(f"| {step['name']} | {step['status']} | {step['summary']} |")
    harness = next((step for step in steps if step["name"] == "Harness"), None)
    if harness:
        lines.extend(
            [
                "",
                "### Harness per-fixture counts",
                "",
                "| Fixture | Status | Golden | Processor | Identical | Missing | Extra | Duplicate | Changed | DLQ Golden | DLQ Processor | DLQ Missing | DLQ Extra | Exit code |",
                "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
            ]
        )
        for item in harness["counts"].get("fixture_counts", []):
            lines.append(
                "| {fixture} | {status} | {golden} | {candidate} | {identical} | {missing} | {extra} | {duplicate} | {changed} | {dlq_golden} | {dlq_candidate} | {dlq_missing} | {dlq_extra} | {exit_code} |".format(
                    **item
                )
            )
    lines.extend(["", "### Findings", ""])
    for step in steps:
        if step["findings"]:
            lines.append(f"**{step['name']}**")
            _append_findings(lines, step["findings"])
    if not any(step["findings"] for step in steps):
        lines.append("No findings.")
    disposition = evidence["disposition"]
    lines.extend(
        [
            "",
            "## Disposition",
            "",
            f"- Privacy: {disposition['privacy_label']} (`{disposition['privacy']}`)",
            f"- Gate verdict: **{disposition['gate_verdict'].upper()}**",
            f"- Red steps: {', '.join(disposition['red_steps']) or 'none'}",
            "",
            "## Residual uncertainties",
            "",
        ]
    )
    lines.extend(f"- {item}" for item in evidence["residual_uncertainties"])
    return "\n".join(lines) + "\n"


def _append_findings(lines: list[str], findings: list[dict]) -> None:
    if not findings:
        lines.append("No manifest changes.")
        return
    for finding in findings[:8]:
        message = finding.get("message", finding.get("type", "finding"))
        context = []
        if finding.get("fixture"):
            context.append(f"fixture {finding['fixture']}")
        if finding.get("topic"):
            context.append(f"topic {finding['topic']}")
        if finding.get("field"):
            context.append(f"field {finding['field']}")
        if finding.get("disposition"):
            context.append(f"disposition {finding['disposition']}")
        lines.append(f"- {message}" + (f" ({', '.join(context)})" if context else ""))
    if len(findings) > 8:
        lines.append(f"- and {len(findings) - 8} more in evidence.json")


def _inventory(manifest: dict) -> dict:
    return {
        "inputs": [
            {
                "topic": topic["topic"],
                "fields": [
                    {
                        "name": field["name"],
                        "classification": field.get("classification"),
                        "purpose": field.get("purpose"),
                    }
                    for field in topic.get("fields", [])
                ],
            }
            for topic in manifest.get("inputs", [])
        ],
        "outputs": [
            {
                "topic": topic["topic"],
                "downstream": topic.get("downstream", []),
                "fields": [
                    {
                        "name": field["name"],
                        "classification": field.get("classification"),
                        "derived_from": field.get("derived_from", []),
                    }
                    for field in topic.get("fields", [])
                ],
            }
            for topic in manifest.get("outputs", [])
        ],
        "logging": manifest.get("logging", {}),
        "retention_days": manifest.get("retention_days"),
    }


def _load_yaml(path: Path) -> dict:
    return yaml.safe_load(path.read_text(encoding="utf-8")) or {}


def _git_value(root: Path, arguments: list[str], default: str) -> str:
    try:
        result = subprocess.run(
            ["git", *arguments],
            cwd=root,
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError:
        return default
    return result.stdout.strip() if result.returncode == 0 else default
