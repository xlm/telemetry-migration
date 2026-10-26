"""Validate Processor records against JSON Schemas and semantic constraints.

Validate each record against its declared JSON Schema, then require
start_ts <= end_ts, nonnegative energy_kwh, event_count >= 1, and exactly one
summary per session_id. Report schema and semantic failures separately and do
not repair or normalize invalid records.
"""
