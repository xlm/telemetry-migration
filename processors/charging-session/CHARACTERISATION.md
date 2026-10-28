# Charging-session characterisation

The Scala Job is the specification. Paths below are relative to
`jobs/charging-session/src/main/scala/example/telemetry/charging/`.

## Input and validation

- Each nonblank JSONL line is parsed independently. Malformed JSON, a non-object
  root, or a non-finite number anywhere inside a declared input field makes the
  entire record corrupt. Unknown fields are ignored. Wrong-typed strings,
  integers and optional numbers become absent, with no coercion. Integers must
  fit a signed 64-bit value. Location is present only if both numeric `lat` and
  `lon` exist. Sources: `model/RawChargingEvent.scala:26-110`,
  `ChargingSessionJob.scala:48-51`.
- Required fields are `event_id`, `vehicle_id`, `event_ts`, `charging_state`,
  `ingest_ts` and `seq`. Presence, not nonempty text or nonnegative numbers, is
  required. `session_id` is additionally required except for `HEARTBEAT`.
  `schema_version`, `soc_pct`, `power_kw` and `location` are optional.
  Sources: `Validation.scala:15-31`, `model/RawChargingEvent.scala:85-94`.
- Validation chooses the first failure in this order: corrupt record
  (`MISSING_FIELD`, both identifiers null), missing required field
  (`MISSING_FIELD`), unsupported state (`UNKNOWN_STATE`), missing non-heartbeat
  session (`MISSING_FIELD`), SOC outside inclusive [0, 100]
  (`SOC_OUT_OF_RANGE`), negative power (`NEGATIVE_POWER`). Noncorrupt failures
  preserve any parsed event and vehicle identifiers. Sources:
  `Validation.scala:15-55`.
- States match exactly: `PLUGGED_IN`, `CHARGING`, `PAUSED`, `COMPLETE`,
  `UNPLUGGED`, `HEARTBEAT`. No case folding or whitespace trimming. Optional
  wrong-typed SOC or power is absent, not rejected. Sources:
  `Validation.scala:6-13,24-31`, `model/RawChargingEvent.scala:85-94`.
- Only valid events reach state, so invalid events cannot advance the Vehicle
  Clock or reserve dedup keys. Location and input schema version are discarded
  during validation. Sources: `ChargingSessionJob.scala:52-65`,
  `Validation.scala:33-45`, `model/ChargingEvent.scala:4-14`.

## Ordering, state, deduplication and closing

- State is keyed by vehicle, then open sessions by session ID. The Vehicle
  Clock starts at signed-64-bit minimum and advances to max(clock, event time).
  Closed session IDs and accepted dedup keys persist without eviction.
  Sources: `ChargingSessionJob.scala:60-65`, `state/VehicleState.scala:5-16`.
- Within each vehicle's invocation, process by `(ingest_ts, event_id)` ascending,
  with stable input order when both tie. This is not event-time order. Sources:
  `SessionStateFunction.scala:24-31`.
- Before processing every valid event, advance the Vehicle Clock and close all
  sessions satisfying `lastEventTs + 1800000 <= clock`, in session-ID order.
  Use the maximum accepted event timestamp as `lastEventTs`, not last arrival.
  A duplicate, late event or heartbeat can therefore trigger a timeout before
  its own handling. Sources: `SessionStateFunction.scala:31-49,66-76`,
  `config/JobConfig.scala:3-4`.
- A validated heartbeat does nothing else. It neither creates/mutates a
  session nor reserves a dedup key nor emits a DLQ record, even if it names a
  closed session or reuses a sequence. Raw heartbeat input still passes normal
  validation. Sources: `Validation.scala:15-31`,
  `SessionStateFunction.scala:32-52,90`.
- The dedup key is `(vehicle_id, seq)`, not event ID or session ID. First
  accepted arrival in processing order wins across all sessions for that
  vehicle. A seen key yields `DUPLICATE` before checking lateness. A closed
  session ID yields `LATE`, regardless of event age. Rejected late keys are
  not reserved. Sources: `util/DedupKey.scala:3`,
  `SessionStateFunction.scala:53-76`.
- Any accepted non-heartbeat state can start a session. Out-of-order event
  times are accepted while it is open. `COMPLETE` and `UNPLUGGED` are appended
  and counted before immediately closing with that reason. A closed session
  never reopens. There is no end-of-input flush. Sources:
  `SessionStateFunction.scala:59-87,93`, `ChargingSessionJob.scala:80-103`.
- Timeouts are event-time only, driven by this vehicle's clock, not wall time,
  ingest time, another vehicle or Spark timeout callbacks. A timeout summary
  ends at the session's maximum accepted event time, not the heartbeat or idle
  deadline. Sources: `ChargingSessionJob.scala:61-65`,
  `SessionStateFunction.scala:31-49`, `Summaries.scala:15-27`.

## Summary and DLQ fields

- Summary fields are exactly those in `model/SessionSummary.scala:3-16`.
  `session_id` is the session map key and `vehicle_id` is the grouping vehicle.
  Events are sorted by `(event_ts, event_id)` ascending for all calculations.
  `start_ts`/`end_ts` are minimum/maximum accepted event time.
  `duration_s` is integer `(end_ts - start_ts) / 1000`, then narrowed to Scala
  `Int`. Sources: `Summaries.scala:9-27`,
  `SessionStateFunction.scala:43,81`.
- `start_soc_pct`/`end_soc_pct` are the first/last present SOC in that sort
  order, not necessarily endpoint events. Both are null if no SOC exists.
  Sources: `Summaries.scala:15-18,28-29`.
- For adjacent sorted events, integrate only when the first event's state is
  `CHARGING`: `0.5 * (firstPower + secondPower) * delta_ms / 3600000`.
  Missing endpoint power is zero. Paused intervals are excluded, but a charging
  interval ending in PAUSED or COMPLETE still uses both endpoint powers.
  Sum double-precision trapezoids in event order, then convert the sum through
  decimal and round once to three decimal places using HALF_EVEN. Sources:
  `Summaries.scala:19,30,38-46`.
- `peak_power_kw` is max(0, every present accepted power), including power on
  noncharging events. `event_count` counts all accepted session events.
  `close_reason` is COMPLETE, UNPLUGGED or TIMEOUT as above. Output
  `schema_version` is always literal `"1"`, ignoring input version.
  Sources: `Summaries.scala:20,31-34`.
- DLQ fields are only `event_id`, `vehicle_id`, `reason`. Reasons are the four
  validation reasons above plus DUPLICATE and LATE. Nullable identifiers are
  serialized as explicit nulls. No location is emitted. Sources:
  `model/DlqRecord.scala:3-7`, `Validation.scala:20-31,52-55`,
  `SessionStateFunction.scala:53-63`, `ChargingSessionJob.scala:29`.
- Golden sessions sort by session ID. DLQ sorts by event ID (null as empty),
  vehicle ID (null as empty), then reason. Sources:
  `ChargingSessionJob.scala:83-103`.

## Logging and Baseline check

- The Job has no application event logging or raw-record logging. Its console
  is stderr, WARN and higher, with timestamp, severity, logger, message and
  exception. Sources: `ChargingSessionJob.scala:16-140`,
  `SessionStateFunction.scala:12-95`, `Validation.scala:5-56`,
  `Summaries.scala:8-48`,
  `jobs/charging-session/src/main/resources/log4j2.properties:1-9`.
- The Processor adapter retains the scaffold's structured emission/rejection
  and run-count logging. Vehicle identifiers use the existing logging utility's
  first 12 lowercase hex characters of SHA-256. Location and raw events must
  never be logged.
  This is within the Baseline's `logging.hashed: [vehicle_id]` and
  `logging.forbidden: [location]`, not a claim that Scala emits those logs.
- Checked `manifests/charging-session.job.yaml`: all 11 parsed input fields,
  all 12 summary fields, and all 3 DLQ fields are declared. Location covers
  its nested coordinates. `_corrupt_record` is an internal parser marker,
  not an input field read from JSON. There is no undeclared data handling.

## Resolved ambiguities and known limitations

- “Late” means closed-session membership, not comparison with a watermark or
  the clock. Timeout runs first, including at the exact idle boundary. Dedup
  takes precedence over LATE. The code determines each of these choices.
- The file adapter treats one input file as one batch and sorts valid events
  by vehicle, ingest timestamp and event ID to match the Job's fixture
  invocation. Calls across separate batches cannot retroactively reorder an
  earlier batch. Cross-vehicle output ordering is not a business rule.
- State and accepted events remain in memory without pruning, like the Job.
  No restart recovery or Kafka adapter is supplied by this Migration.
- Ordinary timestamp subtraction/addition follows Scala signed-64-bit wrap,
  and duration narrows to signed 32-bit. Contract-valid telemetry avoids those
  overflow extremes. Unbounded double energy can also overflow for extreme
  numeric input, as in Scala. Decimal rounding uses the shortest decimal
  representation of a finite double, not binary fixed-point rounding.
- The adapter uses Serde JSON, while the Job uses Jackson. JSON parser edge
  cases outside the fixtures are not exhaustively compared. In particular,
  Rust rejects overflow in a declared field as it parses it, while Jackson's
  non-finite check runs after constructing the complete object. A later
  duplicate field could overwrite an earlier overflowing value in Jackson.
