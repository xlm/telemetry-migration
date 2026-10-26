"""Compare Processor output with a Golden and report contract-level drift.

Pair summary records by session_id and compare numeric values using
math.isclose with abs_tol=1e-6. Compare Dead-Letter Queue contents as a
multiset of (event_id, reason) pairs so duplicate records remain significant.
Report missing, extra, and mismatched records without changing either input.
"""
