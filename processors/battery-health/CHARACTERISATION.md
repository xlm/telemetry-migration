# Battery health characterisation

Specification:
`jobs/battery-health/src/main/scala/example/telemetry/battery/BatteryHealthJob.scala`
(line references below refer to this file unless stated otherwise). No earlier
Rust implementation was consulted.

## Validation and DLQ

Invariant: each nonblank input line produces exactly one health record or one
DLQ record (lines 103-112, 33-67).

- Input must parse as a JSON object. Parse failures, nonobjects, and nonfinite
  floating-point numbers anywhere within a declared input field (nested values
  included) produce `MISSING_FIELD` with both identifiers null. Unknown extra
  fields, such as `location`, are not read or inspected for nonfinite numbers
  (lines 16-18, 34-44, 72-75).
- Jackson's tree parser (`mapper.readTree`, line 35) retains the final value
  when a JSON object key repeats. Duplicate keys do not by themselves cause
  rejection. The Processor's local deserialiser keeps the same final-value
  rule for typed fields, including a final null or wrong-typed value.
- Required text fields are `event_id`, `vehicle_id` and `battery_id`. Missing,
  null or non-text values are absent. Empty and whitespace text is present and
  not trimmed (lines 46-48, 69-70).
- `soh_pct` must be a JSON number (integral or floating point), not a quoted
  number (line 49). `event_ts` must be an integral JSON number representable as
  a signed 64-bit integer, including zero and negative values. Floating-point
  values such as `1.5`, quoted timestamps and values beyond `i64::MAX` do not
  qualify (lines 50-52).
- Any absent required field yields `MISSING_FIELD`, retaining each correctly
  typed `event_id` and `vehicle_id` independently (lines 53-56). This is the
  only DLQ reason.
- Input `schema_version` is not required or validated. It takes part in the
  declared-field nonfinite check (lines 16-18, 37), but output always carries
  the constant `"1"` (line 63).

## Health classification

- `soh_pct` is clamped with `max(0.0, min(100.0, soh))` (line 58). Negative
  zero becomes positive zero (Java `Math.max(0.0, -0.0)` is `0.0`).
- Health uses the clamped value: `>= 80.0` is `HEALTHY`, `>= 60.0` is
  `DEGRADED`, otherwise `REPLACE` (lines 59-61). Boundaries 60 and 80 are
  inclusive in the higher band.
- The emitted `soh_pct` is the clamped double with no rounding (lines 58, 62-63).
  Aggregation formulas, precision and rounding modes beyond this clamp: not
  applicable.

## Output, ordering, and state

- A health record carries the original text `event_id`, `vehicle_id` and
  `battery_id`, the clamped `soh_pct`, `health`, the unchanged `event_ts` and
  `schema_version` `"1"` (lines 21-29, 62-64).
- A DLQ record contains only nullable `event_id`, nullable `vehicle_id` and
  `reason` (lines 31, 44, 56).
- Classification is independent per line (lines 106-112). Dedup key,
  tie-break, state key, close conditions, Vehicle Clock, late-event rejection,
  timeout and heartbeat semantics: not applicable. Duplicate event ids and
  decreasing timestamps are neither rejected nor coalesced. The output
  timestamp is the input `event_ts`.
- The Scala fixture runner sorts health Goldens by `(event_ts, event_id)` and
  DLQ Goldens by `(event_id or "", vehicle_id or "", reason)` (lines 117-123),
  processes fixtures in filename order (lines 91-95), skips blank lines
  (line 103) and writes newline-terminated JSONL (lines 132-135). These are
  Golden materialisation rules, not core rules. The Processor's JSONL wrapper
  emits in input order and the Harness compares records per key.

## Baseline and privacy

- Scala reads only `event_id`, `vehicle_id`, `battery_id`, `soh_pct`,
  `event_ts` and `schema_version` (lines 16-18, 46-52). Its two output shapes
  match the health and DLQ fields of `manifests/battery-health.job.yaml`
  (lines 21-31). No undeclared data handling was found.
- Scala classification has no application logging of event data (lines
  33-75). Its runtime config sends WARN-level infrastructure logs to stderr
  (`jobs/battery-health/src/main/resources/log4j2.properties`, lines 3-9). The
  Baseline permits only hashed `vehicle_id` in logs, forbids location, retains
  data for 30 days and limits destinations to health for maintenance and DLQ
  for ops.
- The pure core performs no I/O or logging. The wrapper logs accepted and
  rejected events through `telemetry_core::logging`, which hashes
  `vehicle_id`, plus a run summary of counts.
- REHEARSAL DEFECT, deliberately seeded and unfixed: `src/main.rs` also prints
  each accepted event's raw `vehicle_id` to stderr. This violates the Baseline
  `logging.hashed: [vehicle_id]` rule and is expected to make the Gate privacy
  step red with `raw-vehicle-id-in-log`. The Processor Manifest still declares
  hashed-only logging, so the leak is a privacy violation, not a Manifest or
  behavioural change.

## Known limitations and ambiguities

- Jackson's `readTree` accepts trailing content after the first JSON value by
  default, whereas the shared JSONL source rejects such a line and the
  Processor emits `MISSING_FIELD` with null identifiers. No fixture exercises
  this, and the Jackson default was not confirmed against the exact Spark
  bundled version.
- Jackson reads an out-of-range float such as `1e400` as Infinity, while
  `serde_json` rejects it. In a declared field both reject the line
  identically. Inside a duplicate declared key whose final value is valid,
  Scala accepts the line and the Processor rejects it.
- Spark trims with `String.trim` (characters up to U+0020) to skip blank lines,
  while the JSONL source uses Unicode whitespace. Lines made only of other
  Unicode whitespace may be skipped by one side and rejected by the other.
