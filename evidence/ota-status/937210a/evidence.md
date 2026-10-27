# Evidence Pack: ota-status @ 937210a

## Identity

- Processor: `ota-status`
- Commit SHA: `937210a00bcbef46c302067b0d7a94701c5e8c90`
- Worktree dirty: `false`
- Base: `origin/main`
- Manifest versions: Baseline `1`, Processor `1`
- Timestamp (UTC): `2026-10-27T12:02:48.675274Z`
- Reproduce: `make gate PROCESSOR=ota-status`

## Inventory

### Inputs

| Topic | Field | Classification | Purpose |
| --- | --- | --- | --- |
| vehicle.ota.events | event_id | internal-operational | status-projection |
| vehicle.ota.events | vehicle_id | vehicle-identifying | status-projection |
| vehicle.ota.events | campaign_id | internal-operational | status-projection |
| vehicle.ota.events | status | internal-operational | status-normalization |
| vehicle.ota.events | progress_pct | internal-operational | progress-clamping |
| vehicle.ota.events | event_ts | internal-operational | status-projection |
| vehicle.ota.events | schema_version | public | contract-version |

### Outputs

| Topic | Downstream System | Field | Classification | Derived from |
| --- | --- | --- | --- | --- |
| vehicle.ota.status | ota | event_id | internal-operational | event_id |
| vehicle.ota.status | ota | vehicle_id | vehicle-identifying | vehicle_id |
| vehicle.ota.status | ota | campaign_id | internal-operational | campaign_id |
| vehicle.ota.status | ota | status | internal-operational | status |
| vehicle.ota.status | ota | progress_pct | internal-operational | progress_pct |
| vehicle.ota.status | ota | event_ts | internal-operational | event_ts |
| vehicle.ota.status | ota | schema_version | public | schema_version |
| vehicle.ota.dlq | ops | event_id | internal-operational | event_id |
| vehicle.ota.dlq | ops | vehicle_id | vehicle-identifying | vehicle_id |
| vehicle.ota.dlq | ops | reason | internal-operational | status |

- Retention: `30` days
- Logging restrictions: forbidden `location`, hashed `vehicle_id`

## Change assessment

No manifest changes.

## Validation

| Step | Status | Summary |
| --- | --- | --- |
| Harness | green | 3 fixtures, 14 identical records, 0 with Drift |
| Contract | green | 14 records, 0 schema failures, 0 semantic failures |
| Downstream System | skipped | no Downstream System script for this Processor's output (downstream: ota) |
| Privacy | green | disposition no-material-change, 0 findings |
| Path check | green | 0 protected paths touched (base origin/main) |

### Harness per-fixture counts

| Fixture | Status | Golden | Processor | Identical | Missing | Extra | Duplicate | Changed | DLQ Golden | DLQ Processor | DLQ Missing | DLQ Extra | Exit code |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 01-normal | green | 4 | 4 | 4 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 02-variant-status-strings | green | 10 | 10 | 10 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 03-malformed | green | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 4 | 4 | 0 | 0 | 0 |

### Findings

No findings.

## Disposition

- Privacy: no material change (`no-material-change`)
- Gate verdict: **GREEN**
- Red steps: none

## Residual uncertainties

- Fixture coverage: equivalence is shown only on the committed fixtures. Rules or inputs no fixture exercises are unverified.
- The runtime scan is value-based, not flow-based. It catches fixture values copied into outputs or logs, not transformed or derived leakage.
- No broker semantics are tested. Ordering across partitions, redelivery, consumer group offsets, and idempotent production are outside the Harness.
