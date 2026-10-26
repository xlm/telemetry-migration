"""Emit one Downstream System warranty flag per charging-session summary.

Use HIGH_ENERGY_PER_SOC when
energy_kwh / max(end_soc - start_soc, 1) > 1.5. Otherwise use TIMEOUT when
close_reason is TIMEOUT, and use OK for every other record. The energy flag
takes precedence over TIMEOUT. Emit exactly one flag per session_id.
"""
