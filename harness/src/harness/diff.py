"""Compare Processor records and Dead-Letter Queue entries with a Golden."""

from __future__ import annotations

import math
from collections import Counter


def _equal(golden, processor) -> bool:
    if isinstance(golden, bool) or isinstance(processor, bool):
        return type(golden) is type(processor) and golden == processor
    if isinstance(golden, (int, float)) and isinstance(processor, (int, float)):
        return math.isclose(golden, processor, rel_tol=0, abs_tol=1e-6)
    if isinstance(golden, dict) and isinstance(processor, dict):
        return golden.keys() == processor.keys() and all(
            _equal(golden[key], processor[key]) for key in golden
        )
    if isinstance(golden, list) and isinstance(processor, list):
        return len(golden) == len(processor) and all(
            _equal(left, right) for left, right in zip(golden, processor, strict=True)
        )
    return golden == processor


def _record_index(records: list[dict], key: str) -> tuple[dict, list]:
    indexed = {}
    duplicates = []
    for record in records:
        value = record.get(key)
        index_key = _freeze(value)
        if index_key in indexed:
            duplicates.append(value)
        else:
            indexed[index_key] = record
    return indexed, duplicates


def compare_records(golden: list[dict], processor: list[dict], key: str) -> dict:
    """Return missing, extra, duplicate, and changed Golden records."""
    golden_by_key, _ = _record_index(golden, key)
    processor_by_key, duplicates = _record_index(processor, key)
    common_keys = golden_by_key.keys() & processor_by_key.keys()
    missing = [
        {
            "key": golden_by_key[item_key].get(key),
            "golden": golden_by_key[item_key],
        }
        for item_key in sorted(golden_by_key.keys() - processor_by_key.keys(), key=repr)
    ]
    extra = [
        {
            "key": processor_by_key[item_key].get(key),
            "processor": processor_by_key[item_key],
        }
        for item_key in sorted(processor_by_key.keys() - golden_by_key.keys(), key=repr)
    ]
    changed = []
    identical = 0
    for item_key in sorted(common_keys, key=repr):
        golden_record = golden_by_key[item_key]
        processor_record = processor_by_key[item_key]
        fields = []
        for field in sorted(golden_record.keys() | processor_record.keys()):
            has_golden = field in golden_record
            has_processor = field in processor_record
            golden_value = golden_record.get(field)
            processor_value = processor_record.get(field)
            if has_golden != has_processor or not _equal(golden_value, processor_value):
                fields.append(
                    {
                        "field": field,
                        "golden": golden_value if has_golden else "<absent>",
                        "processor": processor_value if has_processor else "<absent>",
                    }
                )
        if fields:
            changed.append({"key": golden_record.get(key), "fields": fields})
        else:
            identical += 1
    return {
        "missing": missing,
        "extra": extra,
        "duplicate": duplicates,
        "changed": changed,
        "identical": identical,
    }


def _freeze(value):
    if isinstance(value, dict):
        return tuple(sorted((key, _freeze(item)) for key, item in value.items()))
    if isinstance(value, list):
        return tuple(_freeze(item) for item in value)
    return value


def compare_dlq(golden: list[dict], processor: list[dict]) -> dict:
    """Compare Dead-Letter Queue (event_id, reason) pairs as a multiset."""
    golden_pairs = Counter(
        (_freeze(record.get("event_id")), _freeze(record.get("reason"))) for record in golden
    )
    processor_pairs = Counter(
        (_freeze(record.get("event_id")), _freeze(record.get("reason"))) for record in processor
    )

    def details(counts: Counter, records: list[dict]) -> list[dict]:
        examples = {
            (_freeze(record.get("event_id")), _freeze(record.get("reason"))): record
            for record in records
        }
        return [
            {
                "event_id": examples[pair]["event_id"],
                "reason": examples[pair]["reason"],
                "count": count,
            }
            for pair, count in counts.items()
        ]

    return {
        "missing": details(golden_pairs - processor_pairs, golden),
        "extra": details(processor_pairs - golden_pairs, processor),
    }
