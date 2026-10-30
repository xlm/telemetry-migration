# OTA status Job characterisation

All references below are to `jobs/ota-status/src/main/scala/example/telemetry/ota/OtaStatusJob.scala`.

## Validation and DLQ

- Nonblank JSONL lines are classified independently. Blank lines are skipped (lines 75-85).
- Jackson reads a JSON tree. Parse errors, non-object roots, or non-finite floating-point numbers anywhere inside any of the seven named input fields yield `MISSING_FIELD` with both identifiers null. Unknown fields are not checked for non-finite numbers (lines 17-26, 123-135, 181-184). Duplicate object keys have Jackson's last-value-wins behavior.
- `event_id`, `vehicle_id`, `campaign_id` and `status` must be JSON strings, including empty strings. `progress_pct` must be a JSON number, and `event_ts` an integral JSON number convertible to a signed 64-bit integer. Missing, null or wrong-typed required fields yield `MISSING_FIELD`, preserving the event and vehicle identifiers if they are strings (lines 137-148, 170-179).
- Only after required-field validation, an unrecognised status yields `UNKNOWN_STATUS` with both identifiers preserved (lines 144-153, 186-194). Thus `MISSING_FIELD` takes precedence over `UNKNOWN_STATUS`.
- `schema_version` is not required or type-validated, but its value is included in the non-finite check (lines 17-25, 123-128, 137-146).

## Projection

- Status is lowercased with `Locale.ROOT`, without trimming. `downloading`, `dl`, `download` map to `DOWNLOADING`; `installing`, `inst` to `INSTALLING`; `done`, `complete`, `success`, `succeeded` to `SUCCEEDED`; `failed`, `error` to `FAILED` (lines 186-194).
- Output copies `event_id`, `vehicle_id`, `campaign_id` and `event_ts`; emits the normalised status; clamps numeric `progress_pct` to [0, 100] with `math.max(0.0, math.min(100.0, _))`, where Java's `max` turns `-0.0` into `0.0`; and always emits `schema_version` as the string `1` (lines 150-163). No decimal rounding is specified beyond conversion to `Double` and clamping (lines 160, 173-174).
- The Job sorts Golden status files by `event_ts`, then `event_id`, and Golden DLQ files by `event_id` and `vehicle_id` with nulls treated as empty strings, then `reason` (lines 92-112). The Processor's existing wrapper emits each record in input order (`processors/ota-status/src/main.rs:34-67`). The Harness compares status records by key and DLQ entries as a multiset, not by position (`harness/src/harness/diff.py:25-42,97-104`).

## State and timing

- Deduplication and tie-break: Not applicable. Every nonblank line is classified separately (lines 75-85).
- State key, session close conditions and timeout semantics: Not applicable. Classification has no per-vehicle state or timeout, and output carries the input `event_ts` (lines 123-168).
- Aggregation and aggregation rounding: Not applicable. The Job projects each record independently (lines 75-85, 150-163).

## Privacy and logging

- The Job code makes no explicit per-record log calls. Its logging configuration sets the root logger to WARN and writes to stderr (`jobs/ota-status/src/main/resources/log4j2.properties:3-9`). Framework-generated logging is not specified by this code.
- Valid output contains the original identifiers. DLQ records retain string identifiers when field validation fails or status is unknown, but parsing and non-finite failures set both identifiers to null (lines 28-42, 123-153, 155-158).

## Known limitations and ambiguities

- Baseline lineage note for platform and privacy owners: the Baseline declares DLQ `reason` as derived only from `status` (`manifests/ota-status.job.yaml:25-30`), but the Job also selects `MISSING_FIELD` based on the presence and types of other required fields and on non-finite values in any named input field (lines 123-148). The fixed enum carries no input data. The protected Baseline and identical Processor Manifest were not changed.
- Reviewer note: a downstream consumer relying on Golden file order would instead see input order from the Processor. Relative order among Golden records with equal sort keys is unspecified (lines 92-109).
- The Job configures framework logging but does not define all messages emitted by Spark or its dependencies (`jobs/ota-status/src/main/resources/log4j2.properties:3-9`).
- Unverified decoder difference outside the fixtures: Jackson 2.15 `ObjectMapper.readTree` (line 125) leaves `FAIL_ON_TRAILING_TOKENS` at its default (off), so a line such as `{...valid...} trailing` is likely projected from the first value. The shared JSONL wrapper parses with `serde_json::from_str` (`processors/telemetry-core/src/adapters/jsonl.rs:87`), which rejects trailing content, so the Processor would send `MISSING_FIELD` with null identifiers. Fixing it would mean replacing the shared wrapper, which is out of this Migration's scope.
