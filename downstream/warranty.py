"""This represents an unchanged Downstream System and must never be modified during a Migration.

Return HIGH_ENERGY_PER_SOC when energy_kwh / max(end_soc_pct - start_soc_pct, 1) > 1.5.
Otherwise return TIMEOUT when close_reason is TIMEOUT, or OK for every other summary.
The energy flag takes precedence. A null SOC value makes the delta zero.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


def flag(summary: dict) -> str:
    start_soc = summary.get("start_soc_pct")
    end_soc = summary.get("end_soc_pct")
    if start_soc is None or end_soc is None:
        soc_delta = 0
    else:
        soc_delta = end_soc - start_soc
    if summary["energy_kwh"] / max(soc_delta, 1) > 1.5:
        return "HIGH_ENERGY_PER_SOC"
    if summary["close_reason"] == "TIMEOUT":
        return "TIMEOUT"
    return "OK"


def main() -> None:
    input_path = Path(sys.argv[1])
    for line in input_path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        summary = json.loads(line)
        print(
            json.dumps(
                {"session_id": summary.get("session_id"), "flag": flag(summary)},
                separators=(",", ":"),
                sort_keys=True,
            )
        )


if __name__ == "__main__":
    main()
