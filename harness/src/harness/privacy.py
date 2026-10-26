"""Compare Processor manifest deltas with the Job Baseline and scan runtime data.

Report new or removed fields, classification or purpose changes, new topics or
Downstream Systems, and retention or logging changes. Runtime output keys must
be declared for their topic, and no output value may equal a fixture location
unless that topic declares location. Logs may contain neither fixture location
values nor raw vehicle_id values; the approved 12-hex vehicle_id hash is allowed.
Findings carry one of no-material-change, clarification, violation, or
new-processing. Do not infer undeclared data permissions.
"""
