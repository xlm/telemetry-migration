"""Run every Gate step and report one aggregate verdict."""

from __future__ import annotations

import subprocess
from pathlib import Path, PurePosixPath

import click

from harness.common import PROCESSORS, ROOT, StepResult
from harness.contract import validate_contract
from harness.downstream import run_downstream
from harness.evidence import write_evidence
from harness.privacy import DISPLAY_LABELS, run_privacy
from harness.run_processor import run_harness

PROTECTED_PATHS = (
    "goldens/",
    "fixtures/",
    "manifests/*.job.yaml",
    "harness/",
    "contracts/",
    "jobs/",
    "downstream/",
)


def path_is_protected(path: str, rules: tuple[str, ...] = PROTECTED_PATHS) -> list[str]:
    matches = []
    normalized = PurePosixPath(path).as_posix()
    for rule in rules:
        directory_match = rule.endswith("/") and normalized.startswith(rule)
        manifest_match = (
            rule == "manifests/*.job.yaml"
            and normalized.startswith("manifests/")
            and normalized.count("/") == 1
            and normalized.endswith(".job.yaml")
        )
        if directory_match or manifest_match:
            matches.append(rule)
    return matches


def check_paths(base: str = "origin/main", *, root: Path = ROOT) -> StepResult:
    changed_paths = set()
    commands = (
        ["diff", "--name-only", f"{base}...HEAD"],
        ["diff", "--name-only", "HEAD"],
        ["ls-files", "--others", "--exclude-standard"],
    )
    for arguments in commands:
        try:
            result = subprocess.run(
                ["git", *arguments],
                cwd=root,
                capture_output=True,
                text=True,
                check=False,
            )
        except OSError as error:
            return StepResult(
                "Path check",
                "red",
                f"git path check failed: {error}",
                {"base": base, "protected_paths": 0},
                [{"type": "git-failed", "command": arguments, "message": str(error)}],
            )
        if result.returncode:
            message = result.stderr.strip() or f"git exited {result.returncode}"
            return StepResult(
                "Path check",
                "red",
                f"git path check failed for {base}: {message}",
                {"base": base, "protected_paths": 0},
                [
                    {
                        "type": "git-failed",
                        "command": arguments,
                        "message": message,
                    }
                ],
            )
        changed_paths.update(line for line in result.stdout.splitlines() if line)

    findings = []
    for path in sorted(changed_paths):
        for rule in path_is_protected(path):
            findings.append(
                {
                    "type": "protected-path",
                    "path": path,
                    "rule": rule,
                    "message": f"{path} matches protected path rule {rule}",
                }
            )
    status = "red" if findings else "green"
    summary = f"{len(findings)} protected paths touched (base {base})"
    return StepResult(
        "Path check",
        status,
        summary,
        {"base": base, "protected_paths": len(findings)},
        findings,
    )


def run_gate(
    processor: str,
    base: str = "origin/main",
    bin_path: Path | None = None,
    *,
    root: Path = ROOT,
    goldens_dir: Path | None = None,
    work_dir: Path | None = None,
    evidence_dir: Path | None = None,
) -> tuple[list[StepResult], int]:
    work_dir = work_dir or root / "target" / "gate" / processor
    calls = [
        (
            "Harness",
            lambda: run_harness(
                processor,
                bin_path,
                root=root,
                goldens_dir=goldens_dir,
                work_dir=work_dir,
            ),
        ),
        (
            "Contract",
            lambda: validate_contract(processor, root=root, work_dir=work_dir),
        ),
        (
            "Downstream System",
            lambda: run_downstream(
                processor,
                root=root,
                goldens_dir=goldens_dir,
                work_dir=work_dir,
            ),
        ),
        ("Privacy", lambda: run_privacy(processor, root=root, work_dir=work_dir)),
        ("Path check", lambda: check_paths(base, root=root)),
    ]
    results = []
    for name, call in calls:
        try:
            results.append(call())
        except Exception as error:  # noqa: BLE001
            results.append(
                StepResult(
                    name,
                    "red",
                    f"unexpected error: {error}",
                    {},
                    [{"type": "unexpected-error", "message": str(error)}],
                )
            )
    try:
        results.append(
            write_evidence(
                processor,
                base,
                results,
                root=root,
                evidence_dir=evidence_dir,
            )
        )
    except Exception as error:  # noqa: BLE001
        results.append(
            StepResult(
                "Evidence Pack",
                "red",
                f"unexpected error: {error}",
                {},
                [{"type": "unexpected-error", "message": str(error)}],
            )
        )
    return results, int(any(step.status == "red" for step in results))


def print_report(
    processor: str,
    base: str,
    results: list[StepResult],
    root: Path = ROOT,
) -> None:
    sha = _git_value(root, ["rev-parse", "--short", "HEAD"], "unknown")
    click.echo(f"Gate: {processor} @ {sha} (base {base})")
    for index, step in enumerate(results, start=1):
        click.echo(f"[{index}/6] {step.name:<20} {step.status.upper():<7} {step.summary}")
        if step.name == "Harness":
            for fixture in step.counts.get("fixture_counts", []):
                click.echo(
                    "      "
                    f"{fixture['fixture']}  {fixture['status'].upper()}  "
                    f"records golden={fixture['golden']} candidate={fixture['candidate']} "
                    f"identical={fixture['identical']} missing={fixture['missing']} "
                    f"extra={fixture['extra']} duplicate={fixture['duplicate']} "
                    f"changed={fixture['changed']}  "
                    f"dlq golden={fixture['dlq_golden']} candidate={fixture['dlq_candidate']} "
                    f"missing={fixture['dlq_missing']} extra={fixture['dlq_extra']} "
                    f"exit={fixture['exit_code']}"
                )
                if fixture["status"] == "red":
                    _print_fixture_details(fixture)
            global_findings = [finding for finding in step.findings if not finding.get("fixture")]
            for finding in global_findings:
                click.echo(f"      {finding.get('message', finding.get('type', 'finding'))}")
                if finding.get("stderr"):
                    for line in finding["stderr"].splitlines()[-20:]:
                        click.echo(f"        {line}")
        elif step.status == "red":
            for finding in step.findings[:10]:
                disposition = finding.get("disposition")
                suffix = (
                    f" [{DISPLAY_LABELS[disposition]}]" if disposition in DISPLAY_LABELS else ""
                )
                message = finding.get("message", finding.get("type", "finding"))
                message_lines = str(message).splitlines()
                if len(message_lines) > 1:
                    context = "/".join(
                        str(finding[key]) for key in ("fixture", "side") if finding.get(key)
                    )
                    label = finding.get("type", "finding")
                    message = f"{label} [{context}]: {message_lines[-1]}"
                click.echo(f"      {message}{suffix}")
            if len(step.findings) > 10:
                click.echo(f"      and {len(step.findings) - 10} more findings")
    click.echo(f"Raw Processor output: target/gate/{processor}/")
    red_steps = [step.name for step in results if step.status == "red"]
    click.echo(f"Gate: {'RED - ' + ', '.join(red_steps) if red_steps else 'GREEN'}")


def _print_fixture_details(fixture: dict) -> None:
    diff = next(
        (finding for finding in fixture["findings"] if finding.get("type") == "drift"),
        None,
    )
    if diff is None:
        for finding in fixture["findings"][:10]:
            click.echo(f"        {finding.get('message', finding.get('type', 'finding'))}")
        return
    lines = []
    for item in diff["missing"]:
        lines.append(f"missing {item['key']}")
    for item in diff["extra"]:
        lines.append(f"extra {item['key']}")
    for key in diff["duplicate"]:
        lines.append(f"duplicate {key}")
    for record in diff["changed"]:
        for field in record["fields"]:
            lines.append(
                f"changed {record['key']} {field['field']}: "
                f"Golden {field['golden']}, Processor {field['processor']}"
            )
    for item in diff["dlq_missing"]:
        lines.append(f"dlq missing ({item['event_id']}, {item['reason']}) x{item['count']}")
    for item in diff["dlq_extra"]:
        lines.append(f"dlq extra ({item['event_id']}, {item['reason']}) x{item['count']}")
    lines.extend(
        finding.get("message", finding.get("type", "finding"))
        for finding in fixture["findings"]
        if finding.get("type") != "drift"
    )
    for line in lines[:10]:
        click.echo(f"        {line}")
    if len(lines) > 10:
        click.echo(f"        and {len(lines) - 10} more fixture findings")


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


@click.command()
@click.option(
    "--processor",
    required=True,
    type=click.Choice(sorted(PROCESSORS)),
    help="Processor to validate.",
)
@click.option(
    "--base", default="origin/main", show_default=True, help="Git base for the Path check."
)
@click.option(
    "--bin",
    "bin_path",
    type=click.Path(path_type=Path, dir_okay=False),
    help="Processor binary. Skips the Cargo build when supplied.",
)
def main(processor: str, base: str, bin_path: Path | None) -> None:
    """Run the six Gate steps for one Processor."""
    results, exit_code = run_gate(processor, base=base, bin_path=bin_path)
    print_report(processor, base, results)
    raise SystemExit(exit_code)
