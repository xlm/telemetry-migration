"""Run the warranty Downstream System over Golden and candidate summaries.

Emit one flag per session_id: HIGH_ENERGY_PER_SOC when
energy_kwh / max(end_soc - start_soc, 1) > 1.5, else TIMEOUT when
close_reason == TIMEOUT, else OK. Compare the two result sets and report
"N sessions, M identical flags" without modifying either input.
"""
