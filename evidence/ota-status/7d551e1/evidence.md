# Evidence Pack: ota-status @ 7d551e1

> [!NOTE]
> **GREEN: ready for human review.** Same data, same use as the approved Baseline. Output matches the legacy Job on all 3 test scenarios (14 records).

[`7d551e1`](https://github.com/xlm/telemetry-migration/commit/7d551e183e1b98eadf387294d2085a4f5b3f0d7c) · branch `devin/1793147821-evidence-pack-headline` · Baseline v1 · Processor Manifest v1 · `make gate PROCESSOR=ota-status BASE=HEAD` · Path check base: this commit

| Harness | Contract | Downstream System | Privacy | Path check |
| --- | --- | --- | --- | --- |
| 🟢 green | 🟢 green | ⚪ skipped | 🟢 green | 🟢 green |

## What changed against the Baseline

No change to what data is read, derived, sent, logged or kept.

## Residual uncertainties

What a green Gate does not show:

- Compared on 3 test scenarios (14 records) only. Behaviour that no scenario exercises is not compared.
- The privacy scan looks for exact copies of test values. It would miss location that was rounded, encoded or merged into another field. The Manifest comparison catches declared derivations from location.
- Kafka behaviour (ordering across partitions, redelivery, offsets) is outside this Gate.

<details>
<summary>Reviewer detail</summary>

## Identity

- Processor: `ota-status`
- Commit SHA: `7d551e183e1b98eadf387294d2085a4f5b3f0d7c`
- Commit URL: https://github.com/xlm/telemetry-migration/commit/7d551e183e1b98eadf387294d2085a4f5b3f0d7c
- Branch: `devin/1793147821-evidence-pack-headline`
- Worktree dirty: `false`
- Base: `HEAD`
- Manifest versions: Baseline `1`, Processor `1`
- Timestamp (UTC): `2026-10-28T01:01:24.593407Z`
- Reproduce: `make gate PROCESSOR=ota-status BASE=HEAD`

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
| Path check | green | 0 protected paths touched (base HEAD) |

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

</details>
