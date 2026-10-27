"""Shared Harness paths, Processor metadata, and Gate step results."""

from __future__ import annotations

import json
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Literal

ROOT = Path(__file__).resolve().parents[3]


@dataclass(frozen=True)
class ProcessorSpec:
    name: str
    key: str
    suffix: str
    contract: str
    semantic_assertions: bool = False


PROCESSORS = {
    "charging-session": ProcessorSpec(
        name="charging-session",
        key="session_id",
        suffix="sessions",
        contract="contracts/charging-session.v1.schema.json",
        semantic_assertions=True,
    ),
    "ota-status": ProcessorSpec(
        name="ota-status",
        key="event_id",
        suffix="status",
        contract="contracts/ota-status.v1.schema.json",
    ),
}


@dataclass
class StepResult:
    name: str
    status: Literal["green", "red", "skipped"]
    summary: str
    counts: dict
    findings: list[dict]

    def to_dict(self) -> dict:
        return asdict(self)


def read_jsonl(path: Path) -> tuple[list[dict], list[str]]:
    """Read JSON objects from JSONL and report missing files or invalid lines."""
    records = []
    errors = []
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except (OSError, UnicodeError) as error:
        return [], [f"missing or unreadable file: {path} ({error})"]

    for line_number, line in enumerate(lines, start=1):
        if not line.strip():
            continue
        try:
            record = json.loads(line)
        except json.JSONDecodeError as error:
            errors.append(f"line {line_number}: invalid JSON ({error.msg})")
            continue
        if not isinstance(record, dict):
            errors.append(f"line {line_number}: expected a JSON object")
            continue
        records.append(record)
    return records, errors
