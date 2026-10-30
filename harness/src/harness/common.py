"""Shared Harness paths, Processor metadata, and Gate step results."""

from __future__ import annotations

import json
import math
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
    "battery-health": ProcessorSpec(
        name="battery-health",
        key="event_id",
        suffix="health",
        contract="contracts/battery-health.v1.schema.json",
    ),
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
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        return [], [f"missing or unreadable file: {path} ({error})"]
    return parse_jsonl(text)


def parse_jsonl(text: str) -> tuple[list[dict], list[str]]:
    """Parse JSON objects from JSONL text and report invalid lines."""
    records = []
    errors = []
    # JSONL separates records with "\n" only. str.splitlines() would also split on
    # characters such as U+2028 that are valid inside a JSON string.
    for line_number, line in enumerate(text.split("\n"), start=1):
        if not line.strip():
            continue
        try:
            record = json.loads(
                line,
                parse_constant=_reject_constant,
                parse_float=_parse_finite_float,
                object_pairs_hook=_reject_duplicate_keys,
            )
        except json.JSONDecodeError as error:
            errors.append(f"line {line_number}: invalid JSON ({error.msg})")
            continue
        except ValueError as error:
            errors.append(f"line {line_number}: invalid JSON ({error})")
            continue
        if not isinstance(record, dict):
            errors.append(f"line {line_number}: expected a JSON object")
            continue
        records.append(record)
    return records, errors


def _reject_constant(name: str):
    raise ValueError(f"{name} is not a JSON number")


def _parse_finite_float(text: str) -> float:
    value = float(text)
    if not math.isfinite(value):
        raise ValueError(f"{text} is not a finite JSON number")
    return value


def _reject_duplicate_keys(pairs: list[tuple[str, object]]) -> dict:
    record = {}
    for key, value in pairs:
        if key in record:
            raise ValueError(f"duplicate key {key!r}")
        record[key] = value
    return record
