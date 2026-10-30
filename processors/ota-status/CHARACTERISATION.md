# OTA status characterisation

Specification: `jobs/ota-status/src/main/scala/example/telemetry/ota/OtaStatusJob.scala`
(line references below refer to this file). No earlier Rust implementation was
consulted.

## Validation and DLQ

Invariant: each nonblank input line produces exactly one status or one DLQ
record, with missing/invalid required fields taking precedence over an unknown
status (lines 75-85, 123-167).

- Input must parse as a JSON object. Parse failures, nonobjects, and nonfinite
  floating-point numbers anywhere within a declared input field produce
  `MISSING_FIELD` with both identifiers null. Unknown extra fields are not
  inspected for nonfinite numbers (lines 17-25, 123-135, 181-184).
- Jackson's tree parser retains the final value when JSON object keys repeat
  (`mapper.readTree`, line 125). Duplicate keys therefore do not by themselves
  cause rejection. OTA's local deserialiser preserves the same final-value
  rule for typed fields, including a final null or wrong-typed value.
- Required text fields are `event_id`, `vehicle_id`, `campaign_id`, and `status`.
  Missing, null, or wrong-typed values are absent. Empty text and whitespace
  identifiers are present, not trimmed (lines 137-148, 170-171).
- `progress_pct` must be numeric, not a quoted number. `event_ts` must be an
  integral JSON number representable as a signed 64-bit integer, including zero
  and negative values. Floating-point or quoted timestamps do not qualify
  (lines 141-148, 173-179).
- An absent required field yields `MISSING_FIELD`, retaining each correctly
  typed identifier independently. Otherwise an unrecognised status yields
  `UNKNOWN_STATUS`, also retaining identifiers. These are the only DLQ reasons
  (lines 134-152). An empty status is present but unknown, not missing.
- Input `schema_version` is not required or validated as text by classification.
  It is included in the declared-field nonfinite check, but never determines
  the output version (lines 17-25, 128, 137-162).

## Status and progress

- Status matching lowercases with `Locale.ROOT`. It does not trim whitespace or
  remove punctuation, quotes, separators, or Unicode marks (lines 186-194).
- `downloading`, `dl`, `download` map to `DOWNLOADING` (line 188).
- `installing`, `inst` map to `INSTALLING` (line 189).
- `done`, `complete`, `success`, `succeeded` map to `SUCCEEDED` (lines 190-191).
- `failed`, `error` map to `FAILED` (line 192).
- Progress is `max(0.0, min(100.0, progress))`, including positive zero for
  negative zero. There is no rounding or status-dependent progress override
  (line 160). Aggregation formulas, precision and rounding modes beyond this
  double-precision clamp: not applicable.

## Output, ordering, and state

- A status contains the original text identifiers, canonical status, clamped
  progress, unchanged event timestamp, and constant string `schema_version`
  `"1"`, irrespective of the input version (lines 28-42, 154-163).
- DLQ contains only nullable `event_id`, nullable `vehicle_id`, and `reason`
  (lines 38-42, 134-152).
- Classification is independent per line (lines 75-85, 123-167). Dedup key,
  tie-break, state key, close conditions, Vehicle Clock, late-event rejection,
  timeout, and heartbeat semantics: not applicable. Duplicate and decreasing
  timestamps are not rejected or coalesced.
- The Scala fixture runner sorts status Goldens by `(event_ts, event_id)` and
  DLQ Goldens by `(event_id or "", vehicle_id or "", reason)` (lines 92-112).
  It processes fixture filenames in sorted order (lines 196-203), skips blank
  lines (line 75), and writes newline-terminated JSONL (lines 211-214).
  These are Golden materialisation rules, not a stateful core sorting rule.
  The existing streaming JSONL wrapper emits in input order per destination.
  The Harness compares records per key, not by physical file position.

## Baseline and privacy

- Scala declares only `event_id`, `vehicle_id`, `campaign_id`, `status`,
  `progress_pct`, `event_ts`, and `schema_version` as inputs (lines 17-25).
  Its two output shapes exactly match the status and DLQ fields in
  `manifests/ota-status.job.yaml` (lines 28-42, 134-163).
- Scala classification has no application logging or hashing of event data
  (lines 123-194). Its runtime config sends WARN-level infrastructure logs to
  stderr (`jobs/ota-status/src/main/resources/log4j2.properties`, lines 3-9).
  The Baseline permits only hashed vehicle identifiers in logs, forbids
  location, retains data for 30 days, and limits destinations to status for OTA
  and DLQ for ops. The unchanged Processor Manifest matches these declarations.
- The pure core performs no I/O or logging. The existing wrapper logs only
  closed event names, reasons, counts, and the shared vehicle hash, not raw
  identifiers, location, or payloads. This is stricter than emitting raw
  identifiers to logs. Raw identifiers remain in the declared output/DLQ
  topics as the Scala specifies.
- Baseline check: no undeclared Scala field read, emitted field, or application
  log found. No classification, purpose, retention, or destination is widened.
  No Manifest change is needed.

## Coverage and residual uncertainty

Core tests cover every alias in lower/upper/mixed case, missing fields and DLQ
precedence, present empty text, whitespace and Unicode/punctuation bypasses,
numeric clamp boundaries and fractional precision, output version, signed
timestamps, repeat events, and input immutability. Binary tests cover type
validation, duplicate keys, malformed JSON, nonfinite numbers, blank lines, per-destination
ordering, extra-field isolation, and private logs, alongside the retained
fixture integration test.

No business rule is ambiguous in the Scala. The existing wrapper is a file
adapter, not a Kafka delivery or throughput test. Scala Golden ordering does
not establish a production cross-record ordering guarantee. Extreme JSON
numeric representations and the full Unicode case table are not exhaustively
covered, and hashes do not establish anonymisation. A green Gate is readiness
for human review, not privacy approval or a production cutover decision.
