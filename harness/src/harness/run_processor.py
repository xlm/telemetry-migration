"""Run a Processor against every fixture and compare its output with Goldens."""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

from harness.common import PROCESSORS, ROOT, StepResult, parse_jsonl, read_jsonl
from harness.diff import compare_dlq, compare_records


def run_harness(
    processor: str,
    bin_path: Path | None = None,
    *,
    root: Path = ROOT,
    goldens_dir: Path | None = None,
    work_dir: Path | None = None,
) -> StepResult:
    spec = PROCESSORS[processor]
    goldens_dir = goldens_dir or root / "goldens"
    work_dir = work_dir or root / "target" / "gate" / processor
    if work_dir.exists():
        shutil.rmtree(work_dir)
    work_dir.mkdir(parents=True)

    findings = []
    fixtures = sorted((root / "fixtures" / processor).glob("*.jsonl"))
    golden_dir = goldens_dir / processor
    # Read before any Processor runs, so a Processor cannot rewrite what it is compared with.
    goldens = _snapshot(golden_dir)
    if not fixtures:
        findings.append({"type": "no-fixtures", "message": f"no fixtures in fixtures/{processor}/"})
    expected_goldens = {
        name
        for fixture in fixtures
        for name in (f"{fixture.stem}.{spec.suffix}.jsonl", f"{fixture.stem}.dlq.jsonl")
    }
    for path in goldens:
        if path.name not in expected_goldens:
            findings.append(
                {
                    "type": "orphan-golden",
                    "path": _display(path, root),
                    "message": f"Golden {_display(path, root)} has no fixture",
                }
            )
    binary = bin_path
    build_failed = False
    if binary is None:
        build = [
            "cargo",
            "build",
            "--release",
            "--manifest-path",
            "processors/Cargo.toml",
            "-p",
            processor,
        ]
        try:
            completed = subprocess.run(
                build,
                cwd=root,
                capture_output=True,
                text=True,
                check=False,
            )
            if completed.returncode != 0:
                build_failed = True
                findings.append(
                    {
                        "type": "build-failed",
                        "message": f"Processor build exited {completed.returncode}",
                        "stderr": "\n".join(completed.stderr.splitlines()[-20:]),
                    }
                )
        except OSError as error:
            build_failed = True
            findings.append(
                {
                    "type": "build-failed",
                    "message": f"could not run cargo build: {error}",
                    "stderr": "",
                }
            )
        binary = root / "processors" / "target" / "release" / processor
    elif not binary.is_absolute():
        binary = root / binary

    fixture_counts = []
    for fixture in fixtures:
        stem = fixture.stem
        output_path = work_dir / f"{stem}.out.jsonl"
        dlq_path = work_dir / f"{stem}.dlq.jsonl"
        log_path = work_dir / f"{stem}.log.jsonl"
        stdout_path = work_dir / f"{stem}.stdout.log"
        stderr_path = work_dir / f"{stem}.stderr.log"
        run_findings = []
        exit_code = None
        if build_failed:
            stdout_path.write_text("", encoding="utf-8")
            stderr_path.write_text("", encoding="utf-8")
            run_findings.append(
                {
                    "type": "not-run",
                    "fixture": stem,
                    "message": "Processor not run: build failed",
                }
            )
            record_diff = {
                "missing": [],
                "extra": [],
                "duplicate": [],
                "changed": [],
                "identical": 0,
            }
            dlq_diff = {"missing": [], "extra": []}
            counts = {
                "fixture": stem,
                "golden": 0,
                "candidate": 0,
                "identical": 0,
                "missing": 0,
                "extra": 0,
                "changed": 0,
                "duplicate": 0,
                "dlq_golden": 0,
                "dlq_candidate": 0,
                "dlq_missing": 0,
                "dlq_extra": 0,
                "exit_code": None,
            }
            findings.extend(run_findings)
            fixture_counts.append(
                {
                    **counts,
                    "status": "red",
                    "record_diff": record_diff,
                    "dlq_diff": dlq_diff,
                    "findings": run_findings,
                }
            )
            continue

        stdout = ""
        stderr = ""
        try:
            completed = subprocess.run(
                [
                    str(binary),
                    "--input",
                    str(fixture),
                    "--output",
                    str(output_path),
                    "--dlq",
                    str(dlq_path),
                    "--log",
                    str(log_path),
                ],
                cwd=root,
                capture_output=True,
                text=True,
                timeout=60,
                check=False,
            )
            exit_code = completed.returncode
            stdout = completed.stdout
            stderr = completed.stderr
            if exit_code:
                run_findings.append(
                    {
                        "type": "non-zero-exit",
                        "fixture": stem,
                        "exit_code": exit_code,
                        "stderr": "\n".join(stderr.splitlines()[-20:]),
                    }
                )
        except subprocess.TimeoutExpired as error:
            stdout = _stream_text(error.stdout)
            stderr = _stream_text(error.stderr)
            run_findings.append(
                {"type": "timeout", "fixture": stem, "message": "Processor exceeded 60 seconds"}
            )
        except OSError as error:
            run_findings.append(
                {"type": "processor-unavailable", "fixture": stem, "message": str(error)}
            )

        stdout_path.write_text(_stream_text(stdout), encoding="utf-8")
        stderr_path.write_text(_stream_text(stderr), encoding="utf-8")
        for label, path in (("output", output_path), ("DLQ", dlq_path), ("log", log_path)):
            if not path.is_file():
                run_findings.append(
                    {"type": "missing-file", "fixture": stem, "file": label, "path": str(path)}
                )

        golden_path = golden_dir / f"{stem}.{spec.suffix}.jsonl"
        golden_dlq_path = golden_dir / f"{stem}.dlq.jsonl"
        golden_records, golden_errors = _golden_records(goldens, golden_path)
        golden_dlq, golden_dlq_errors = _golden_records(goldens, golden_dlq_path)
        candidate_records, candidate_errors = read_jsonl(output_path)
        candidate_dlq, candidate_dlq_errors = read_jsonl(dlq_path)
        _, candidate_log_errors = read_jsonl(log_path)

        if golden_path not in goldens:
            run_findings.append(
                {"type": "missing-golden", "fixture": stem, "path": str(golden_path)}
            )
        if golden_dlq_path not in goldens:
            run_findings.append(
                {"type": "missing-golden", "fixture": stem, "path": str(golden_dlq_path)}
            )
        for label, errors in (
            ("Golden", golden_errors),
            ("Golden DLQ", golden_dlq_errors),
            ("Processor output", candidate_errors),
            ("Processor DLQ", candidate_dlq_errors),
            ("Processor log", candidate_log_errors),
        ):
            run_findings.extend(
                {"type": "unparseable-line", "fixture": stem, "file": label, "message": error}
                for error in errors
            )

        record_diff = compare_records(golden_records, candidate_records, spec.key)
        dlq_diff = compare_dlq(golden_dlq, candidate_dlq)
        counts = {
            "fixture": stem,
            "golden": len(golden_records),
            "candidate": len(candidate_records),
            "identical": record_diff["identical"],
            "missing": len(record_diff["missing"]),
            "extra": len(record_diff["extra"]),
            "changed": len(record_diff["changed"]),
            "duplicate": len(record_diff["duplicate"]),
            "dlq_golden": len(golden_dlq),
            "dlq_candidate": len(candidate_dlq),
            "dlq_missing": sum(item["count"] for item in dlq_diff["missing"]),
            "dlq_extra": sum(item["count"] for item in dlq_diff["extra"]),
            "exit_code": exit_code,
        }
        drift = any(
            counts[name]
            for name in (
                "missing",
                "extra",
                "duplicate",
                "changed",
                "dlq_missing",
                "dlq_extra",
            )
        )
        if drift:
            run_findings.append(
                {
                    "type": "drift",
                    "fixture": stem,
                    "counts": counts,
                    **record_diff,
                    "dlq_missing": dlq_diff["missing"],
                    "dlq_extra": dlq_diff["extra"],
                }
            )
        findings.extend(run_findings)
        fixture_counts.append(
            {
                **counts,
                "status": "red" if run_findings else "green",
                "record_diff": record_diff,
                "dlq_diff": dlq_diff,
                "findings": run_findings,
            }
        )

    goldens_after = _snapshot(golden_dir)
    for path in sorted(goldens.keys() | goldens_after.keys()):
        if goldens.get(path) != goldens_after.get(path):
            findings.append(
                {
                    "type": "golden-modified",
                    "path": _display(path, root),
                    "message": f"Golden {_display(path, root)} changed while the Processor ran",
                }
            )

    totals = {
        "fixtures": len(fixtures),
        "identical": sum(item["identical"] for item in fixture_counts),
        "with_drift": sum(
            any(
                item[name]
                for name in (
                    "missing",
                    "extra",
                    "duplicate",
                    "changed",
                    "dlq_missing",
                    "dlq_extra",
                )
            )
            for item in fixture_counts
        ),
        "fixture_counts": fixture_counts,
    }
    status = "red" if findings else "green"
    summary = (
        f"{len(fixtures)} fixtures, {totals['identical']} identical records, "
        f"{totals['with_drift']} with Drift"
    )
    return StepResult("Harness", status, summary, totals, findings)


def _snapshot(directory: Path) -> dict[Path, bytes | None]:
    if not directory.is_dir():
        return {}
    try:
        paths = sorted(directory.iterdir())
    except OSError:
        return {}
    snapshot = {}
    for path in paths:
        if path.is_file():
            try:
                snapshot[path] = path.read_bytes()
            except OSError:
                snapshot[path] = None
    return snapshot


def _golden_records(
    goldens: dict[Path, bytes | None], path: Path
) -> tuple[list[dict], list[str]]:
    if path not in goldens:
        return [], [f"missing or unreadable file: {path}"]
    if goldens[path] is None:
        return [], [f"missing or unreadable file: {path}"]
    try:
        return parse_jsonl(goldens[path].decode("utf-8"))
    except UnicodeError as error:
        return [], [f"missing or unreadable file: {path} ({error})"]


def _display(path: Path, root: Path) -> str:
    try:
        return path.relative_to(root).as_posix()
    except ValueError:
        return str(path)


def _stream_text(stream: str | bytes | None) -> str:
    if stream is None:
        return ""
    return stream.decode("utf-8", errors="replace") if isinstance(stream, bytes) else stream
