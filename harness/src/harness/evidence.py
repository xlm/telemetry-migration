"""Write the structured JSON and readable Markdown Evidence Pack."""

from __future__ import annotations

import json
import os
import re
import shlex
import subprocess
from datetime import UTC, datetime
from pathlib import Path

import yaml

from harness.common import ROOT, StepResult
from harness.privacy import DISPLAY_LABELS

RESIDUAL_UNCERTAINTIES = [
    "Compared on {fixtures} ({records}) only. Behaviour that no scenario exercises is not compared.",
    "The privacy scan looks for exact copies of test values. It would miss location that was rounded, encoded or merged into another field. The Manifest comparison catches declared derivations from location.",
    "Kafka behaviour (ordering across partitions, redelivery, offsets) is outside this Gate.",
]

STEP_NAMES = ("Harness", "Contract", "Downstream System", "Privacy", "Path check")
LEAD_ORDER = ("Path check", "Privacy", "Harness", "Contract", "Downstream System")
DISPOSITION_ORDER = ("violation", "new-processing", "clarification", "no-material-change")
EXECUTIVE_PRIVACY_LABELS = {
    "violation": "policy violation, must be fixed",
    "new-processing": "new processing, needs its own privacy review",
    "clarification": "clarification, reviewer confirms",
}
HARNESS_TYPE_ORDER = (
    "build-failed",
    "non-zero-exit",
    "timeout",
    "processor-unavailable",
    "drift",
)
PRIVACY_RUNTIME_ORDER = (
    "location-in-output",
    "location-in-log",
    "raw-vehicle-id-in-log",
    "undeclared-output-field",
)


def write_evidence(
    processor: str,
    base: str,
    validation_steps: list[StepResult],
    *,
    root: Path = ROOT,
    evidence_dir: Path | None = None,
    bin_path: Path | None = None,
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
        reproduce += f" BASE={shlex.quote(base)}"
    if bin_path is not None:
        reproduce += f" BIN={shlex.quote(_repo_relative_path(bin_path, root))}"
    inventory = _inventory(manifest)
    identity = {
        "processor": processor,
        "sha": sha,
        "short_sha": short_sha,
        "worktree_dirty": dirty,
        "base": base,
        "baseline_manifest_version": baseline.get("version"),
        "processor_manifest_version": manifest.get("version"),
        "timestamp_utc": datetime.now(UTC).isoformat().replace("+00:00", "Z"),
        "reproduce_command": reproduce,
    }
    identity.update(_identity_links(root, sha))
    validation = [step.to_dict() for step in validation_steps[:5]]
    evidence = {
        "identity": identity,
        "inventory": inventory,
        "change_assessment": privacy_findings,
        "validation": validation,
        "disposition": {
            "privacy": privacy_counts.get("disposition", "no-material-change"),
            "privacy_label": privacy_counts.get("label", DISPLAY_LABELS["no-material-change"]),
            "gate_verdict": "red" if red_steps else "green",
            "red_steps": red_steps,
        },
        "residual_uncertainties": _residual_uncertainties(validation),
    }
    evidence["summary"] = _summary(evidence)
    destination = evidence_dir / processor / short_sha
    try:
        destination.mkdir(parents=True, exist_ok=True)
        (destination / "evidence.json").write_text(
            json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        markdown = _markdown(evidence)
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


def _repo_relative_path(path: Path, root: Path) -> str:
    supplied_path = Path(path)
    candidate = supplied_path if supplied_path.is_absolute() else root / supplied_path
    try:
        return candidate.resolve().relative_to(root.resolve()).as_posix()
    except (OSError, RuntimeError, ValueError):
        return str(path)


def _identity_links(root: Path, sha: str) -> dict:
    server_url = os.environ.get("GITHUB_SERVER_URL", "").strip()
    repository = os.environ.get("GITHUB_REPOSITORY", "").strip()
    repo_url = None
    if server_url and repository:
        repo_url = f"{server_url.rstrip('/')}/{repository.removesuffix('.git')}"
    else:
        remote = _git_value(root, ["remote", "get-url", "origin"], "")
        match = re.search(r"github\.com[:/]([^/\s:]+/[^/\s]+)", remote)
        if match:
            repo = match.group(1).removesuffix(".git").rstrip("/")
            repo_url = f"https://github.com/{repo}"

    head_ref = os.environ.get("GITHUB_HEAD_REF", "").strip()
    branch = head_ref or _git_value(root, ["rev-parse", "--abbrev-ref", "HEAD"], "")
    if not branch or branch == "HEAD":
        branch = None
    event_name = os.environ.get("GITHUB_EVENT_NAME", "")
    github_ref = os.environ.get("GITHUB_REF", "")
    pull_request = re.fullmatch(r"refs/pull/(\d+)/merge", github_ref)
    pr_number = (
        int(pull_request.group(1)) if event_name == "pull_request" and pull_request else None
    )
    commit_url = f"{repo_url}/commit/{sha}" if repo_url and sha and sha != "unknown" else None
    return {
        "commit_url": commit_url,
        "branch": branch,
        "pr_number": pr_number,
    }


def _residual_uncertainties(validation: list[dict]) -> list[str]:
    harness = next((step for step in validation if step["name"] == "Harness"), None)
    if harness is None:
        coverage = "Compared on the committed test scenarios only. Behaviour that no scenario exercises is not compared."
    else:
        counts = harness["counts"]
        fixtures = _plural(
            counts.get("fixtures", len(counts.get("fixture_counts", []))), "test scenario"
        )
        records = _plural(
            sum(item.get("golden", 0) for item in counts.get("fixture_counts", [])),
            "record",
        )
        coverage = RESIDUAL_UNCERTAINTIES[0].format(fixtures=fixtures, records=records)
    return [coverage, *RESIDUAL_UNCERTAINTIES[1:]]


def _summary(evidence: dict) -> dict:
    validation = evidence["validation"]
    validation_by_name = {step["name"]: step for step in validation}
    statuses = {
        name: validation_by_name[name]["status"] if name in validation_by_name else "not run"
        for name in STEP_NAMES
    }
    red_steps = [name for name in LEAD_ORDER if statuses[name] == "red"]
    verdict = "red" if red_steps else "green"
    privacy = validation_by_name.get("Privacy", {})
    privacy_findings = privacy.get("findings", [])
    clarifications = sum(
        finding.get("disposition") == "clarification" for finding in privacy_findings
    )
    alert = "CAUTION" if verdict == "red" else "IMPORTANT" if clarifications else "NOTE"
    lead_step = red_steps[0] if red_steps else None
    headline = (
        _red_headline(
            lead_step,
            red_steps,
            validation,
            evidence["change_assessment"],
            evidence["inventory"],
        )
        if lead_step
        else _green_headline(validation, clarifications)
    )
    baseline_changes = [
        {
            "type": finding.get("type", "unknown"),
            "disposition": finding.get("disposition"),
            "sentence": _baseline_change_sentence(finding, evidence["inventory"]),
        }
        for finding in evidence["change_assessment"]
        if finding.get("type") != "version-change"
    ]
    runtime_privacy = any(finding.get("check") == "runtime" for finding in privacy_findings)
    baseline_unchanged = None
    if not baseline_changes:
        baseline_unchanged = (
            "No declared change. The Processor Manifest matches the Baseline, but the output does not (see What failed)."
            if runtime_privacy
            else "No change to what data is read, derived, sent, logged or kept."
        )
    failures = []
    for name in LEAD_ORDER:
        if statuses[name] != "red":
            continue
        step = validation_by_name.get(name, {})
        findings = step.get("findings", [])
        if name == "Privacy":
            failures.extend(_privacy_failures(findings, evidence["inventory"]))
        else:
            failures.append(_step_failure(name, step, validation, evidence["inventory"]))
    return {
        "verdict": verdict,
        "alert": alert,
        "verdict_label": (
            "GREEN: ready for human review."
            if verdict == "green"
            else "RED: not ready for human review."
        ),
        "headline": headline,
        "lead_step": lead_step,
        "steps": [{"name": name, "status": statuses[name]} for name in STEP_NAMES],
        "baseline_unchanged": baseline_unchanged,
        "baseline_changes": baseline_changes,
        "failures": failures,
    }


def _green_headline(validation: list[dict], clarifications: int) -> str:
    harness = next((step for step in validation if step["name"] == "Harness"), None)
    counts = harness["counts"] if harness else {}
    fixtures = counts.get("fixtures", len(counts.get("fixture_counts", [])))
    records = sum(item.get("golden", 0) for item in counts.get("fixture_counts", []))
    fixture_text = _plural(fixtures, "test scenario")
    record_text = _plural(records, "record")
    if clarifications:
        clarification_text = _plural(clarifications, "clarification")
        return (
            f"Output matches the legacy Job on all {fixture_text} ({record_text}), "
            f"with {clarification_text} for the reviewer to confirm."
        )
    return (
        f"Same data, same use as the approved Baseline. Output matches the legacy Job on all "
        f"{fixture_text} ({record_text})."
    )


def _red_headline(
    lead_step: str,
    red_steps: list[str],
    validation: list[dict],
    changes: list[dict],
    inventory: dict,
) -> str:
    step = next((item for item in validation if item["name"] == lead_step), {})
    reason = _lead_reason(lead_step, step, validation, changes, inventory).rstrip(".")
    other_red = [name for name in LEAD_ORDER if name in red_steps and name != lead_step]
    suffix = f" Also red: {', '.join(other_red)}." if other_red else ""
    return f"Blocked at {lead_step}: {reason}.{suffix}"


def _lead_reason(
    name: str, step: dict, validation: list[dict], changes: list[dict], inventory: dict
) -> str:
    findings = step.get("findings", [])
    if name == "Path check":
        protected = [finding for finding in findings if finding.get("type") == "protected-path"]
        if protected:
            paths = [finding.get("path", "no detail") for finding in protected]
            return (
                f"this branch changes {_plural(len(set(paths)), 'Gate input')}, e.g. "
                f"`{_value(paths[0])}`, so the other results cannot be trusted"
            )
        failed = next(
            (finding for finding in findings if finding.get("type") == "git-failed"), None
        )
        if failed:
            return f"the Path check could not run ({_value(failed.get('message', 'no detail'))})"
    if name == "Privacy" and findings:
        disposition_order = {item: index for index, item in enumerate(DISPOSITION_ORDER)}
        selected = min(
            enumerate(findings),
            key=lambda item: (
                disposition_order.get(item[1].get("disposition"), len(DISPOSITION_ORDER)),
                0 if item[1].get("check") == "runtime" else 1,
                item[0],
            ),
        )[1]
        if selected.get("check") == "manifest":
            match = next(
                (
                    finding
                    for finding in changes
                    if finding.get("type") == selected.get("type")
                    and finding.get("field") == selected.get("field")
                    and finding.get("topic") == selected.get("topic")
                ),
                selected,
            )
            sentence = _baseline_change_sentence(match, inventory).rstrip(".")
            return sentence[0].lower() + sentence[1:]
        if selected.get("type") == "location-in-output":
            topic = selected.get("topic")
            downstream = _downstream_for_topic(inventory, topic)
            sent = f", sent to {_join(downstream)}" if downstream else ""
            return (
                f"location appears in `{_value(selected.get('field', 'unknown'))}` on "
                f"`{_value(topic or 'unknown')}`{sent}, which the Baseline does not allow"
            )
        sentence = _privacy_finding_sentence(selected, findings, inventory)
        return sentence.strip().rstrip(".")
    if name == "Harness":
        return _harness_lead_reason(step, validation)
    if name == "Contract":
        if findings:
            records = len({(item.get("fixture"), _value(item.get("key"))) for item in findings})
            return f"{_count_noun_verb(records, 'output record', 'breaks', 'break')} the output contract"
        return _fallback_sentence("unknown", []).rstrip(".")
    if name == "Downstream System":
        flag_drift = [item for item in findings if item.get("type") == "flag-drift"]
        if flag_drift:
            sessions = sum(
                len(item.get("missing", []))
                + len(item.get("extra", []))
                + len(item.get("changed", []))
                for item in flag_drift
            )
            return (
                f"warranty flags differ for {_plural(sessions, 'session')} in "
                f"{_plural(_finding_fixtures(flag_drift), 'test scenario')}"
            )
        crashes = [item for item in findings if item.get("type") == "downstream-crash"]
        if crashes:
            return (
                f"the warranty script crashed on "
                f"{_plural(_finding_fixtures(crashes), 'test scenario')}"
            )
        sentences, _types = _downstream_sentences(findings, validation)
        return (
            sentences[0].rstrip(".") if sentences else _fallback_sentence("unknown", []).rstrip(".")
        )
    kind = findings[0].get("type", "unknown") if findings else "unknown"
    return _fallback_sentence(kind, findings).rstrip(".")


def _harness_lead_reason(step: dict, validation: list[dict]) -> str:
    findings = step.get("findings", [])
    fixtures = _harness_total(validation)
    build_failed = any(item.get("type") == "build-failed" for item in findings)
    if build_failed:
        return "the Processor did not build, so nothing could be compared with the legacy Job"
    crashes = [
        item
        for item in findings
        if item.get("type") in ("non-zero-exit", "timeout", "processor-unavailable")
    ]
    crash_fixtures = {item.get("fixture") for item in crashes if item.get("fixture")}
    if fixtures and len(crash_fixtures) == fixtures:
        return (
            "the Processor crashed on all "
            + _plural(fixtures, "test scenario")
            + ", so none of its output could be compared with the legacy Job"
        )
    if crash_fixtures:
        return f"the Processor crashed on {len(crash_fixtures)} of {fixtures} test scenarios"
    drifts = [item for item in findings if item.get("type") == "drift"]
    drift_fixtures = {item.get("fixture") for item in drifts if item.get("fixture")}
    if drifts and all(item.get("type") == "drift" for item in findings):
        return f"output differs from the legacy Job in {len(drift_fixtures)} of {fixtures} test scenarios"
    sentences, _types = _harness_sentences(findings, validation)
    return sentences[0].rstrip(".") if sentences else _fallback_sentence("unknown", []).rstrip(".")


def _harness_sentences(findings: list[dict], validation: list[dict]) -> tuple[list[str], list[str]]:
    types = []
    sentences = []
    ordered = [
        kind for kind in HARNESS_TYPE_ORDER if any(item.get("type") == kind for item in findings)
    ]
    ordered.extend(
        kind
        for kind in dict.fromkeys(item.get("type", "unknown") for item in findings)
        if kind not in HARNESS_TYPE_ORDER
    )
    total = _harness_total(validation)
    for kind in ordered:
        matches = [item for item in findings if item.get("type", "unknown") == kind]
        types.append(kind)
        if kind == "build-failed":
            sentences.append("the Processor did not build, so no scenario ran.")
        elif kind == "non-zero-exit":
            codes = sorted({str(item.get("exit_code", "unknown")) for item in matches})
            fixtures = _finding_fixtures(matches)
            sentences.append(
                f"the Processor crashed (exit {', '.join(codes)}) on {fixtures} of {total} scenarios."
            )
        elif kind == "timeout":
            sentences.append(
                f"the Processor ran past 60 seconds on {_finding_fixtures(matches)} of {total} scenarios."
            )
        elif kind == "processor-unavailable":
            sentences.append(
                f"the Processor could not start on {_finding_fixtures(matches)} of {total} scenarios."
            )
        elif kind == "drift":
            sentences.append(_drift_sentence(matches, total))
        else:
            sentences.append(_fallback_sentence(kind, matches))
    return sentences, types


def _drift_sentence(findings: list[dict], total: int) -> str:
    fixture_count = _finding_fixtures(findings)
    counts = [finding.get("counts", {}) for finding in findings]
    missing = sum(item.get("missing", 0) for item in counts)
    extra = sum(item.get("extra", 0) for item in counts)
    changed = sum(item.get("changed", 0) for item in counts)
    duplicate = sum(item.get("duplicate", 0) for item in counts)
    dlq_missing = sum(item.get("dlq_missing", 0) for item in counts)
    dlq_extra = sum(item.get("dlq_extra", 0) for item in counts)
    all_empty = bool(counts) and all(
        item.get("candidate", 0) == 0 and item.get("dlq_candidate", 0) == 0 for item in counts
    )
    if all_empty and (missing or dlq_missing):
        parts = []
        if missing:
            parts.append(_plural(missing, "record"))
        if dlq_missing:
            parts.append(_plural(dlq_missing, "dead-letter entry"))
        verb = "is" if len(parts) == 1 and (missing or dlq_missing) == 1 else "are"
        return f"{' and '.join(parts)} that the legacy Job produces {verb} missing."
    parts = []
    if changed:
        parts.append(_count_noun_verb(changed, "record", "differs", "differ"))
    if missing:
        parts.append(f"{missing} missing")
    if extra:
        parts.append(f"{extra} extra")
    if duplicate:
        parts.append(f"{_plural(duplicate, 'record')} duplicated")
    if dlq_missing:
        parts.append(f"{_plural(dlq_missing, 'dead-letter entry')} missing")
    if dlq_extra:
        parts.append(f"{_plural(dlq_extra, 'dead-letter entry')} extra")
    example = _drift_example(findings)
    if changed and not any((missing, extra, duplicate, dlq_missing, dlq_extra)):
        body = f"{parts[0]} from the legacy Job in {fixture_count} of {total} scenarios"
    else:
        body = (
            f"{', '.join(parts)} compared with the legacy Job, in "
            f"{fixture_count} of {total} scenarios"
        )
    return f"{body}, e.g. {example}."


def _drift_example(findings: list[dict]) -> str:
    for finding in findings:
        for changed in finding.get("changed", []):
            fields = changed.get("fields", [])
            if fields:
                field = fields[0]
                return (
                    f"`{_value(changed.get('key'))}` `{_value(field.get('field'))}`: "
                    f"legacy {_value(field.get('golden'))}, new {_value(field.get('processor'))}"
                )
    for finding in findings:
        if finding.get("missing"):
            return f"`{_value(finding['missing'][0].get('key'))}` is missing"
    for finding in findings:
        if finding.get("extra"):
            return f"`{_value(finding['extra'][0].get('key'))}` is extra"
    for finding in findings:
        if finding.get("duplicate"):
            return f"`{_value(finding['duplicate'][0])}` appears more than once"
    for field in ("dlq_missing", "dlq_extra"):
        for finding in findings:
            if finding.get(field):
                item = finding[field][0]
                return (
                    f"dead-letter `{_value(item.get('event_id'))}` ({_value(item.get('reason'))})"
                )
    return "no detail"


def _contract_sentences(findings: list[dict]) -> tuple[list[str], list[str]]:
    types = []
    sentences = []
    for kind in ("schema", "semantic"):
        matches = [item for item in findings if item.get("type") == kind]
        if not matches:
            continue
        types.append(kind)
        records = len({(item.get("fixture"), _value(item.get("key"))) for item in matches})
        example = matches[0]
        prefix = _count_noun_verb(records, "record", "breaks", "break")
        prefix += " the output schema" if kind == "schema" else " a contract rule"
        detail = f"`{_value(example.get('key'))}`"
        if kind == "schema":
            detail += f" `{_value(example.get('field'))}`"
        detail += f": {_value(example.get('message', 'no detail'))}"
        sentences.append(f"{prefix}, e.g. {detail}.")
    other = [item for item in findings if item.get("type") not in ("schema", "semantic")]
    fallback, fallback_types = _fallback_sentences(other)
    return sentences + fallback, types + fallback_types


def _downstream_sentences(
    findings: list[dict], validation: list[dict]
) -> tuple[list[str], list[str]]:
    sentences = []
    types = []
    flag_drift = [item for item in findings if item.get("type") == "flag-drift"]
    if flag_drift:
        types.append("flag-drift")
        missing = sum(len(item.get("missing", [])) for item in flag_drift)
        extra = sum(len(item.get("extra", [])) for item in flag_drift)
        changed = [change for item in flag_drift for change in item.get("changed", [])]
        sessions = missing + extra + len(changed)
        fixtures = _finding_fixtures(flag_drift)
        total = _harness_total(validation)
        sentence = (
            f"warranty flags differ for {_plural(sessions, 'session')} in "
            f"{fixtures} of {total} scenarios."
        )
        if missing and not extra and not changed:
            sentence += " A session missing from the output is a lost warranty flag."
        elif changed:
            item = changed[0]
            sentence += (
                f" e.g. `{_value(item.get('session_id'))}`: legacy "
                f"{_value(item.get('golden'))}, new {_value(item.get('processor'))}."
            )
        sentences.append(sentence)
    for kind, sentence_template in (
        ("downstream-crash", "the warranty script crashed on {fixtures}."),
        ("duplicate-session", "warranty output repeated a session in {fixtures}."),
    ):
        matches = [item for item in findings if item.get("type") == kind]
        if matches:
            types.append(kind)
            sentences.append(
                sentence_template.format(fixtures=_plural(_finding_fixtures(matches), "scenario"))
            )
    other = [
        item
        for item in findings
        if item.get("type") not in ("flag-drift", "downstream-crash", "duplicate-session")
    ]
    fallback, fallback_types = _fallback_sentences(other)
    return sentences + fallback, types + fallback_types


def _privacy_failures(findings: list[dict], inventory: dict) -> list[dict]:
    failures = []
    for disposition in DISPOSITION_ORDER:
        group = [item for item in findings if item.get("disposition") == disposition]
        if not group:
            continue
        ordered_types = [
            kind
            for kind in PRIVACY_RUNTIME_ORDER
            if any(item.get("type") == kind for item in group)
        ]
        ordered_types.extend(
            kind
            for kind in dict.fromkeys(
                item.get("type", "unknown") for item in group if item.get("check") != "manifest"
            )
            if kind not in PRIVACY_RUNTIME_ORDER
        )
        sentences = []
        types = []
        for kind in ordered_types:
            matches = [
                item
                for item in group
                if item.get("type", "unknown") == kind and item.get("check") != "manifest"
            ]
            types.append(kind)
            sentences.append(_privacy_type_sentence(kind, matches, inventory))
        declared = [
            item
            for item in group
            if item.get("check") == "manifest" and item.get("type") != "version-change"
        ]
        if declared:
            types.extend(dict.fromkeys(item.get("type", "unknown") for item in declared))
            sentences.append(f"{_plural(len(declared), 'declared change')} listed above.")
        count = sum(max(1, item.get("count", 1)) for item in group)
        failures.append(
            {
                "step": "Privacy",
                "types": list(dict.fromkeys(types)),
                "disposition": disposition,
                "count": count,
                "sentence": " ".join(sentence for sentence in sentences if sentence),
            }
        )
    other = [item for item in findings if item.get("disposition") not in DISPOSITION_ORDER]
    if other:
        sentences, types = _fallback_sentences(other)
        failures.append(
            {
                "step": "Privacy",
                "types": types,
                "disposition": None,
                "count": len(other),
                "sentence": " ".join(sentences),
            }
        )
    return failures


def _privacy_finding_sentence(finding: dict, findings: list[dict], inventory: dict) -> str:
    kind = finding.get("type", "unknown")
    matches = [item for item in findings if item.get("type", "unknown") == kind]
    if finding.get("check") == "manifest":
        return _baseline_change_sentence(finding, inventory)
    return _privacy_type_sentence(kind, matches, inventory)


def _privacy_type_sentence(kind: str, findings: list[dict], inventory: dict) -> str:
    if kind == "location-in-output" and findings:
        finding = findings[0]
        topic = finding.get("topic")
        field = finding.get("field")
        count = sum(item.get("count", 1) for item in findings)
        fixture = finding.get("fixture", "unknown")
        sentence = (
            f"`{_value(topic or 'unknown')}` carries location in `{_value(field or 'unknown')}` "
            f"in {_plural(count, 'record')}, first in scenario `{_value(fixture)}`."
        )
        downstream = _downstream_for_topic(inventory, topic)
        if downstream:
            sentence += f" {_capitalized_join(downstream)} {_verb_for_count(len(downstream), 'read')} this topic."
        if any(str(item.get("message", "")).endswith("Baseline unchanged") for item in findings):
            sentence += " The Baseline does not allow location there."
        return sentence
    if kind == "location-in-log" and findings:
        count = sum(item.get("count", 1) for item in findings)
        example = findings[0].get("example", "no detail")
        return f"logs carry a location coordinate on {_plural(count, 'line')}, e.g. `{_value(example)}`."
    if kind == "raw-vehicle-id-in-log" and findings:
        count = sum(item.get("count", 1) for item in findings)
        example = findings[0].get("example", "no detail")
        return f"logs carry a raw vehicle ID on {_plural(count, 'line')}, e.g. `{_value(example)}`."
    if kind == "undeclared-output-field" and findings:
        finding = findings[0]
        count = sum(item.get("count", 1) for item in findings)
        return (
            f"`{_value(finding.get('topic', 'unknown'))}` emits "
            f"`{_value(finding.get('field', 'unknown'))}`, which the Processor Manifest does not "
            f"declare, in {_plural(count, 'record')}."
        )
    return _fallback_sentence(kind, findings)


def _baseline_change_sentence(finding: dict, inventory: dict) -> str:
    kind = finding.get("type", "unknown")
    field = _value(finding.get("field", "unknown"))
    topic = _value(finding.get("topic", "unknown"))
    processor = finding.get("processor")
    baseline = finding.get("baseline")
    if kind == "new-output-field":
        downstream = _downstream_for_topic(inventory, finding.get("topic"))
        read_by = f", read by {_join(downstream)}" if downstream else ""
        sentence = (
            f"the Processor Manifest adds `{field}` ({_value(processor or 'unknown')}) "
            f"to `{topic}`{read_by}."
        )
    elif kind == "new-input-field":
        sentence = (
            f"the Processor Manifest reads a new field `{field}` "
            f"({_value(processor or 'unknown')}) from `{topic}`."
        )
    elif kind == "new-downstream":
        sentence = (
            f"the Processor Manifest sends `{topic}` to a new Downstream System, "
            f"{_value(processor or 'unknown')}."
        )
    elif kind == "classification-change":
        sentence = (
            f"the Processor Manifest changes `{field}` on `{topic}` from "
            f"{_value(baseline or 'unknown')} to {_value(processor or 'unknown')}."
        )
    else:
        message = _value(finding.get("message", "no detail")).rstrip(".")
        sentence = f"Processor Manifest: {message}."
    return sentence[0].upper() + sentence[1:]


def _step_failure(name: str, step: dict, validation: list[dict], inventory: dict) -> dict:
    findings = step.get("findings", [])
    if not findings:
        sentences, types = _fallback_sentences([{"type": "unknown"}])
    elif name == "Harness":
        sentences, types = _harness_sentences(findings, validation)
    elif name == "Contract":
        sentences, types = _contract_sentences(findings)
    elif name == "Downstream System":
        sentences, types = _downstream_sentences(findings, validation)
    elif name == "Path check":
        sentences, types = _path_check_sentences(findings)
    else:
        sentences, types = _fallback_sentences(findings)
    return {
        "step": name,
        "types": types,
        "disposition": None,
        "count": len(findings),
        "sentence": " ".join(sentences),
    }


def _path_check_sentences(findings: list[dict]) -> tuple[list[str], list[str]]:
    protected = [item for item in findings if item.get("type") == "protected-path"]
    sentences = []
    types = []
    if protected:
        paths = list(dict.fromkeys(item.get("path", "no detail") for item in protected))
        displayed = [f"`{_value(path)}`" for path in paths[:8]]
        if len(paths) > 8:
            displayed.append(f"and {len(paths) - 8} more")
        sentences.append(
            f"this branch changes {_plural(len(paths), 'Gate input')}: {', '.join(displayed)}."
        )
        types.append("protected-path")
    other = [item for item in findings if item.get("type") != "protected-path"]
    fallback, fallback_types = _fallback_sentences(other)
    return sentences + fallback, types + fallback_types


def _harness_counts(validation: list[dict]) -> dict:
    harness = next((step for step in validation if step["name"] == "Harness"), None)
    return harness.get("counts", {}) if harness else {}


def _harness_total(validation: list[dict]) -> int:
    counts = _harness_counts(validation)
    return counts.get("fixtures", len(counts.get("fixture_counts", [])))


def _finding_fixtures(findings: list[dict]) -> int:
    fixtures = {finding.get("fixture") for finding in findings if finding.get("fixture")}
    return len(fixtures) or len(findings)


def _downstream_for_topic(inventory: dict, topic: str | None) -> list[str]:
    return next(
        (
            item.get("downstream", [])
            for item in inventory.get("outputs", [])
            if item["topic"] == topic
        ),
        [],
    )


def _join(values: list[str]) -> str:
    if len(values) < 2:
        return ", ".join(values)
    if len(values) == 2:
        return f"{values[0]} and {values[1]}"
    return f"{', '.join(values[:-1])} and {values[-1]}"


def _capitalized_join(values: list[str]) -> str:
    joined = _join(values)
    return joined[:1].upper() + joined[1:]


def _fallback_sentences(findings: list[dict]) -> tuple[list[str], list[str]]:
    kinds = list(dict.fromkeys(item.get("type", "unknown") for item in findings))
    return (
        [
            _fallback_sentence(
                kind, [item for item in findings if item.get("type", "unknown") == kind]
            )
            for kind in kinds
        ],
        kinds,
    )


def _fallback_sentence(kind: str, findings: list[dict]) -> str:
    example = "no detail"
    if findings:
        finding = findings[0]
        detail = finding.get("message") or finding.get("path")
        if detail is not None:
            example = _value(detail)
    return f"{_plural(len(findings), f'`{kind}` finding')}, e.g. {example}."


def _plural(count: int, noun: str) -> str:
    if count == 1:
        return f"1 {noun}"
    plural = "dead-letter entries" if noun == "dead-letter entry" else f"{noun}s"
    return f"{count} {plural}"


def _count_noun_verb(count: int, noun: str, singular: str, plural: str) -> str:
    return f"{_plural(count, noun)} {singular if count == 1 else plural}"


def _verb_for_count(count: int, singular: str) -> str:
    if count == 1:
        return f"{singular}s"
    return singular


def _value(value) -> str:
    if isinstance(value, (dict, list)):
        text = json.dumps(value, ensure_ascii=False, separators=(",", ":"))
    else:
        text = str(value)
    return text if len(text) <= 60 else f"{text[:59]}…"


def _markdown(evidence: dict) -> str:
    identity = evidence["identity"]
    processor = identity["processor"]
    short_sha = identity["short_sha"]
    summary = evidence["summary"]
    identity_parts = []
    short_commit = f"`{short_sha}`"
    if identity.get("commit_url"):
        short_commit = f"[`{short_sha}`]({identity['commit_url']})"
    identity_parts.append(short_commit)
    if identity.get("branch"):
        identity_parts.append(f"branch `{identity['branch']}`")
    if identity.get("pr_number") is not None and identity.get("commit_url"):
        repo_url = identity["commit_url"].split("/commit/", 1)[0]
        identity_parts.append(
            f"PR [#{identity['pr_number']}]({repo_url}/pull/{identity['pr_number']})"
        )
    if identity.get("baseline_manifest_version") is not None:
        identity_parts.append(f"Baseline v{identity['baseline_manifest_version']}")
    if identity.get("processor_manifest_version") is not None:
        identity_parts.append(f"Processor Manifest v{identity['processor_manifest_version']}")
    if identity.get("reproduce_command"):
        identity_parts.append(f"`{identity['reproduce_command']}`")
    if identity.get("base") == "HEAD":
        identity_parts.append("Path check base: this commit")
    status_icons = {
        "green": "🟢 green",
        "red": "🔴 red",
        "skipped": "⚪ skipped",
        "not run": "⚪ not run",
    }
    step_statuses = {item["name"]: item["status"] for item in summary["steps"]}
    lines = [
        f"# Evidence Pack: {processor} @ {short_sha}",
        "",
        f"> [!{summary['alert']}]",
        f"> **{summary['verdict_label']}** {summary['headline']}",
        "",
        " · ".join(identity_parts),
        "",
        "| Harness | Contract | Downstream System | Privacy | Path check |",
        "| --- | --- | --- | --- | --- |",
        "| " + " | ".join(status_icons[step_statuses[name]] for name in STEP_NAMES) + " |",
        "",
        "## What changed against the Baseline",
        "",
    ]
    if summary["baseline_unchanged"]:
        lines.append(summary["baseline_unchanged"])
    else:
        for item in summary["baseline_changes"]:
            label = EXECUTIVE_PRIVACY_LABELS.get(
                item["disposition"], item["disposition"] or "unclassified"
            )
            lines.append(f"- {item['sentence']} *({label})*")
    if summary["failures"]:
        lines.extend(["", "## What failed", ""])
        for item in summary["failures"]:
            step_label = item["step"]
            if item["step"] == "Privacy":
                label = EXECUTIVE_PRIVACY_LABELS.get(item["disposition"], "unclassified")
                step_label = f"Privacy - {label}"
            lines.append(f"- **{step_label}:** {item['sentence']}")
    lines.extend(
        [
            "",
            "## Residual uncertainties",
            "",
            "What a green Gate does not show:",
            "",
        ]
    )
    lines.extend(f"- {item}" for item in evidence["residual_uncertainties"])
    lines.extend(["", "<details>", "<summary>Reviewer detail</summary>", ""])
    lines.extend(_reviewer_markdown(evidence))
    lines.extend(["", "</details>"])
    return "\n".join(lines) + "\n"


def _reviewer_markdown(evidence: dict) -> list[str]:
    identity = evidence["identity"]
    inventory = evidence["inventory"]
    steps = evidence["validation"]
    processor = identity["processor"]
    lines = [
        "## Identity",
        "",
        f"- Processor: `{processor}`",
        f"- Commit SHA: `{identity['sha']}`",
    ]
    if identity.get("commit_url"):
        lines.append(f"- Commit URL: {identity['commit_url']}")
    if identity.get("branch"):
        lines.append(f"- Branch: `{identity['branch']}`")
    if identity.get("pr_number") is not None:
        lines.append(f"- PR: `#{identity['pr_number']}`")
    lines.extend(
        [
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
    )
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
        ]
    )
    return lines


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
