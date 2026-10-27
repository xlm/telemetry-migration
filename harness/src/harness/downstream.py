"""Compare warranty Downstream System flags on Golden and Processor output."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import yaml

from harness.common import PROCESSORS, ROOT, StepResult


def run_downstream(
    processor: str,
    *,
    root: Path = ROOT,
    goldens_dir: Path | None = None,
    work_dir: Path | None = None,
) -> StepResult:
    spec = PROCESSORS[processor]
    goldens_dir = goldens_dir or root / "goldens"
    work_dir = work_dir or root / "target" / "gate" / processor
    baseline = yaml.safe_load(
        (root / "manifests" / f"{processor}.job.yaml").read_text(encoding="utf-8")
    )
    applicable = any(
        "warranty" in topic.get("downstream", []) and not topic["topic"].endswith(".dlq")
        for topic in baseline.get("outputs", [])
    )
    if not applicable:
        destinations = sorted(
            {
                destination
                for topic in baseline.get("outputs", [])
                if not topic["topic"].endswith(".dlq")
                for destination in topic.get("downstream", [])
            }
        )
        return StepResult(
            "Downstream System",
            "skipped",
            "no Downstream System script for this Processor's output "
            f"(downstream: {', '.join(destinations) or 'none'})",
            {"sessions": 0, "identical_flags": 0},
            [],
        )

    warranty = root / "downstream" / "warranty.py"
    findings = []
    sessions = 0
    identical_flags = 0
    for fixture in sorted((root / "fixtures" / processor).glob("*.jsonl")):
        stem = fixture.stem
        paths = {
            "Golden": goldens_dir / processor / f"{stem}.{spec.suffix}.jsonl",
            "Processor": work_dir / f"{stem}.out.jsonl",
        }
        flag_sets = {}
        for side, input_path in paths.items():
            try:
                completed = subprocess.run(
                    [sys.executable, str(warranty), str(input_path)],
                    cwd=root,
                    capture_output=True,
                    text=True,
                    check=False,
                )
                if completed.returncode:
                    findings.append(
                        {
                            "type": "downstream-crash",
                            "fixture": stem,
                            "side": side,
                            "message": "\n".join(completed.stderr.splitlines()[-20:]),
                        }
                    )
                    flag_sets[side] = {}
                    continue
                outputs = [
                    json.loads(line) for line in completed.stdout.splitlines() if line.strip()
                ]
                flag_sets[side] = {item["session_id"]: item["flag"] for item in outputs}
                if len(flag_sets[side]) != len(outputs):
                    findings.append(
                        {
                            "type": "duplicate-session",
                            "fixture": stem,
                            "side": side,
                            "message": "warranty output repeated a session_id",
                        }
                    )
            except (OSError, json.JSONDecodeError) as error:
                findings.append(
                    {
                        "type": "downstream-crash",
                        "fixture": stem,
                        "side": side,
                        "message": str(error),
                    }
                )
                flag_sets[side] = {}

        golden_flags = flag_sets.get("Golden", {})
        processor_flags = flag_sets.get("Processor", {})
        session_ids = golden_flags.keys() | processor_flags.keys()
        sessions += len(session_ids)
        identical_flags += sum(
            golden_flags.get(session_id) == processor_flags.get(session_id)
            for session_id in session_ids
        )
        if golden_flags != processor_flags:
            findings.append(
                {
                    "type": "flag-drift",
                    "fixture": stem,
                    "missing": sorted(golden_flags.keys() - processor_flags.keys()),
                    "extra": sorted(processor_flags.keys() - golden_flags.keys()),
                    "changed": [
                        {
                            "session_id": session_id,
                            "golden": golden_flags[session_id],
                            "processor": processor_flags[session_id],
                        }
                        for session_id in golden_flags.keys() & processor_flags.keys()
                        if golden_flags[session_id] != processor_flags[session_id]
                    ],
                }
            )

    status = "red" if findings or identical_flags < sessions else "green"
    summary = f"{sessions} sessions, {identical_flags} identical flags"
    return StepResult(
        "Downstream System",
        status,
        summary,
        {"sessions": sessions, "identical_flags": identical_flags},
        findings,
    )
