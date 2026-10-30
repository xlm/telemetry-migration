# charging-session characterisation

The Scala Job under `jobs/charging-session/` is the specification. Paths below
are relative to `jobs/charging-session/src/main/scala/example/telemetry/charging/`.
Line numbers refer to the Job as of the commit this file was written on.

## Input parsing

- Blank lines are dropped before parsing (`ChargingSessionJob.scala:50`).
- Each line is parsed with Jackson `readTree` (`model/RawChargingEvent.scala:41-50`).
  The line is corrupt when it is not valid JSON, is not a JSON object, or a
  known input field (`event_id` ... `schema_version`, nested values included)
  holds a non-finite number such as `1e400` (`RawChargingEvent.scala:26-38,44-49,96-99`).
  A non-finite number under an unknown key is ignored (Job test
  `ChargingSessionSpec.scala`, `overflowInUnknownField`).
- Duplicate object keys: `readTree` keeps the last value (Jackson default).
  The non-finite check applies only to the value left after last-wins, at every
  nesting level.
- Field typing is exact JSON type, a wrong-typed or null value is absent
  (`RawChargingEvent.scala:85-94`):
  - text fields (`event_id`, `vehicle_id`, `session_id`, `charging_state`,
    `schema_version`) must be JSON strings;
  - `event_ts`, `ingest_ts`, `seq` must be JSON integers that fit a signed
    64-bit long (`1.0` is absent);
  - `soc_pct`, `power_kw` accept any JSON number, read as a double;
  - `location` is an object with numeric `lat` and `lon`, otherwise absent
    (`RawChargingEvent.scala:101-109`). It is parsed but never used.

## Validation and DLQ reasons

First failing check wins (`Validation.scala:15-50`):

1. corrupt line -> `MISSING_FIELD`, with `event_id` and `vehicle_id` null
   (`Validation.scala:20-21`);
2. any of `event_id`, `vehicle_id`, `event_ts`, `charging_state`, `ingest_ts`,
   `seq` absent -> `MISSING_FIELD` (`Validation.scala:16-18,22-23`);
3. `charging_state` not exactly one of `PLUGGED_IN`, `CHARGING`, `PAUSED`,
   `COMPLETE`, `UNPLUGGED`, `HEARTBEAT` (case-sensitive) -> `UNKNOWN_STATE`
   (`Validation.scala:6-13,24-25`);
4. `session_id` absent and state is not `HEARTBEAT` -> `MISSING_FIELD`
   (`Validation.scala:26-27`);
5. `soc_pct` present and `< 0.0` or `> 100.0` -> `SOC_OUT_OF_RANGE`; the bounds
   are inclusive and the check applies to heartbeats too (`Validation.scala:28-29`);
6. `power_kw` present and `< 0.0` -> `NEGATIVE_POWER`; `-0.0` passes
   (`Validation.scala:30-31`).

A validation DLQ record carries the raw `event_id` and `vehicle_id` when they
parsed, otherwise null (`Validation.scala:52-56`). Absent `soc_pct` and
`power_kw` are valid.

## State key, ordering and Vehicle Clock

- State key: `vehicle_id` (`ChargingSessionJob.scala:60-65`). One state per
  vehicle: Vehicle Clock, open sessions by `session_id`, closed session ids,
  seen dedup keys (`state/VehicleState.scala:5-17`).
- Valid events of one vehicle in a micro-batch are processed sorted by
  `(ingest_ts, event_id)` (`SessionStateFunction.scala:31`). Golden generation
  reads each fixture as one `AvailableNow` batch (`ChargingSessionJob.scala:48-85`).
- Vehicle Clock: starts at `Long.MinValue`, becomes `max(clock, event_ts)` for
  every valid event, heartbeats, duplicates and late events included
  (`SessionStateFunction.scala:32`, `VehicleState.scala:11-12,16`). Event time
  only, never processing time.

## Timeout semantics

- After advancing the clock, before anything else is done with the event,
  every open session with `lastEventTs + 30 min <= clock` closes with
  `TIMEOUT`, in ascending `session_id` order (`SessionStateFunction.scala:33-50`,
  `config/JobConfig.scala:4`). `lastEventTs` is the highest `event_ts` among the
  session's accepted events (`SessionStateFunction.scala:68,72`).
- The closed id joins the closed list. The summary's `vehicle_id` is the
  current event's (same vehicle).
- `GroupStateTimeout.NoTimeout` (`ChargingSessionJob.scala:64`): there is no
  end-of-input flush. A session still open when input ends emits nothing.
- A session opened by an event whose `event_ts` is already older than the
  clock by 30 min survives that event and times out on the next valid event.

## Dedup, late and session updates

For a non-heartbeat valid event (`SessionStateFunction.scala:52-90`):

1. dedup key `(vehicle_id, seq)` already seen -> DLQ `DUPLICATE`
   (`util/DedupKey.scala:3`, `SessionStateFunction.scala:53-58`). The first
   accepted event wins, a different `event_id` does not matter. Keys are
   remembered for the vehicle's lifetime, across sessions;
2. else `session_id` is in the closed list -> DLQ `LATE` (`SessionStateFunction.scala:59-63`);
3. else the event joins its session (created on first event), and its dedup
   key is remembered (`SessionStateFunction.scala:65-77`). DUPLICATE and LATE
   events do not record a dedup key.
4. if the state is `COMPLETE` or `UNPLUGGED`, the session closes with that
   state as `close_reason` and joins the closed list (`SessionStateFunction.scala:79-88`).

Heartbeats only advance the clock and trigger timeouts. They are never
deduplicated, never added to a session, never counted
(`SessionStateFunction.scala:52`). They can still be rejected by validation.
`PLUGGED_IN`, `CHARGING` and `PAUSED` just join the session.

## Summary aggregation

Built from the session's accepted events sorted by `(event_ts, event_id)`
(`Summaries.scala:15`):

- `session_id`: the session key; `vehicle_id`: the vehicle (`Summaries.scala:23-24`).
- `start_ts` / `end_ts`: min / max `event_ts` (`Summaries.scala:16-17`).
- `duration_s`: `(end_ts - start_ts) / 1000`, integer division truncating,
  narrowed to a 32-bit int (`Summaries.scala:27`).
- `start_soc_pct` / `end_soc_pct`: `soc_pct` of the first / last sorted event
  that has one, null when none has (`Summaries.scala:18,28-29`).
- `energy_kwh`: trapezoid over each consecutive sorted pair whose first event
  is `CHARGING`: `0.5 * (p1 + p2) * (t2 - t1) / 3_600_000`, absent power
  counts as 0.0 (`Summaries.scala:38-47`). The double total goes through
  `BigDecimal(double)` (decimal string of the double) and is rounded to 3
  decimals `HALF_EVEN` (`Summaries.scala:30`). Example: 0.6625 -> 0.662.
- `peak_power_kw`: max of present `power_kw` values, starting from 0.0, so
  0.0 when none is present (`Summaries.scala:20`). Not rounded.
- `event_count`: number of accepted events in the session (`Summaries.scala:32`).
- `close_reason`: `COMPLETE`, `UNPLUGGED` or `TIMEOUT` (`Summaries.scala:33`).
- `schema_version`: the literal `"1"`, not the input's (`Summaries.scala:34`).

## Outputs

- Sessions topic fields as listed above, nulls written explicitly
  (`ChargingSessionJob.scala:29`, `model/SessionSummary.scala`).
- DLQ topic fields: `event_id`, `vehicle_id`, `reason` (`model/DlqRecord.scala`).
- Golden files are sorted (`ChargingSessionJob.scala:83-100`); output order is
  not part of the contract, the Harness compares per key.

## Logging and identifiers

- The Job code logs nothing itself. Spark logs at `WARN` to stderr
  (`src/main/resources/log4j2.properties`). No identifier or location is
  logged by Job code.
- The Processor logs one JSON line per emitted summary and per DLQ record with
  `vehicle_id` hashed (`telemetry_core::logging`), and a run summary of counts.
  It never logs `location`, as the Baseline `logging` section requires.

## Rules that do not apply

- No processing-time timers, no watermark, no output-time timestamp beyond
  `start_ts` / `end_ts` from event time.
- No location use: `location` is read (declared, purpose `none`) and dropped.

## Known limitations and ambiguities

- Ordering: the Job sorts a vehicle's events by `(ingest_ts, event_id)` within
  a micro-batch. The Processor handles events in arrival order, which matches
  when input arrives in that order, as in every fixture. Input that arrives out
  of `ingest_ts` order within one Spark micro-batch would be handled
  differently.
- JSON edge cases the fixtures do not exercise, where the Rust line parser
  (`serde_json`) and Jackson differ: trailing content after the object (Jackson
  `readTree` ignores it by default, the Processor rejects the line as corrupt),
  and integer literals too large for a double (Jackson keeps a big integer,
  the Processor rejects the line as corrupt).
- Rounding uses Rust's shortest round-trip decimal for the double. JDK 17
  `Double.toString` is not always shortest, which can only matter for a total
  whose extra digits sit on a half-way point at the fourth decimal.
- Spark crashes on a non-finite energy total (`BigDecimal(Infinity)`); the
  Processor does not reproduce a crash. Not reachable with finite inputs of
  realistic magnitude.
