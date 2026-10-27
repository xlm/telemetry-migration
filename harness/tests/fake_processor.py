#!/usr/bin/env python3
"""Stdlib-only OTA Processor used to exercise the Python Harness."""

import argparse
import hashlib
import json
from pathlib import Path

ALIASES = {
    "downloading": "DOWNLOADING",
    "dl": "DOWNLOADING",
    "download": "DOWNLOADING",
    "installing": "INSTALLING",
    "inst": "INSTALLING",
    "done": "SUCCEEDED",
    "complete": "SUCCEEDED",
    "success": "SUCCEEDED",
    "succeeded": "SUCCEEDED",
    "failed": "FAILED",
    "error": "FAILED",
}


def _line(record):
    return json.dumps(record, separators=(",", ":"), sort_keys=True) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--dlq", required=True, type=Path)
    parser.add_argument("--log", required=True, type=Path)
    args = parser.parse_args()

    output = []
    dlq = []
    logs = []
    for line in args.input.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            event = {}
        if not isinstance(event, dict):
            event = {}
        event_id = event.get("event_id")
        vehicle_id = event.get("vehicle_id")
        status = event.get("status")
        progress = event.get("progress_pct")
        event_ts = event.get("event_ts")
        missing = (
            not isinstance(event_id, str)
            or not isinstance(vehicle_id, str)
            or not isinstance(event.get("campaign_id"), str)
            or not isinstance(status, str)
            or not isinstance(progress, (int, float))
            or isinstance(progress, bool)
            or not isinstance(event_ts, int)
            or isinstance(event_ts, bool)
        )
        if missing:
            reason = "MISSING_FIELD"
            dlq.append(
                {
                    "event_id": event_id if isinstance(event_id, str) else None,
                    "vehicle_id": vehicle_id if isinstance(vehicle_id, str) else None,
                    "reason": reason,
                }
            )
            event_name = "event_rejected"
        else:
            normalized = ALIASES.get(status.lower())
            if normalized is None:
                reason = "UNKNOWN_STATUS"
                dlq.append(
                    {
                        "event_id": event_id if isinstance(event_id, str) else None,
                        "vehicle_id": vehicle_id if isinstance(vehicle_id, str) else None,
                        "reason": reason,
                    }
                )
                event_name = "event_rejected"
            else:
                reason = ""
                output.append(
                    {
                        "event_id": event_id,
                        "vehicle_id": vehicle_id,
                        "campaign_id": event["campaign_id"],
                        "status": normalized,
                        "progress_pct": float(min(max(progress, 0), 100)),
                        "event_ts": event_ts,
                        "schema_version": "1",
                    }
                )
                event_name = "status_emitted"
        vehicle_id_hash = (
            hashlib.sha256(vehicle_id.encode("utf-8")).hexdigest()[:12]
            if isinstance(vehicle_id, str)
            else ""
        )
        logs.append(
            {
                "level": "INFO",
                "fields": {
                    "processor": "ota_status",
                    "event": event_name,
                    "vehicle_id_hash": vehicle_id_hash,
                    "reason": reason,
                },
                "target": "telemetry_core::logging",
            }
        )

    for path, records in (
        (args.output, output),
        (args.dlq, dlq),
        (args.log, logs),
    ):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("".join(_line(record) for record in records), encoding="utf-8")


if __name__ == "__main__":
    main()
