# Cutover

Cutover is the step of a Migration where Downstream Systems stop receiving a
Job's output and start receiving its Processor's. It comes after the Gate. A
green Gate means the Processor is ready for human review, not that it is live.
Every step on this page is a human decision.

This page describes the process. Nothing in this repository runs it yet:
Processors read and write files, and `deploy/` defines no services.

## Who does what

| Step | Who | Done when |
| --- | --- | --- |
| Migration PR | Devin | `3. Gate verdict` is green on the PR head commit |
| Review | Engineering and privacy reviewers | Evidence Pack read, PR approved and merged |
| Shadow | Platform operator | Processor output matches Job output on live input |
| Switch | Platform operator | The Processor is the only writer to the output topics |
| Retire | Platform team | Rollback window closed, Job deployment removed |

Devin never approves, merges, switches or rolls back.

## 1. Shadow

Run the Processor next to the Job on the same input topic, with its own
consumer group. It writes to shadow topics that no Downstream System reads.
Compare each shadow record with the Job's record for the same key, as the
Harness does with Goldens. Any difference is a Drift, and its fix goes
through a new Migration PR and the Gate like any other.

Shadow topics are new destinations. The Gate's privacy check calls a new
topic `new-processing`, and the same applies here: get privacy sign-off
before creating them, carry only the Baseline's fields, and keep them no
longer than the Baseline's `retention_days`.

## 2. Switch

Keep exactly one writer on each output topic.

1. Stop the Job.
2. Start the Processor on the real output topics, from an input offset early
   enough to rebuild the state the Job held in memory. charging-session keeps
   a session open until it closes or sits idle for 30 minutes of Vehicle
   Clock, so start before the first event of the oldest open session.
3. Downstream Systems keep reading the same topics under the same contract,
   so they change nothing.

Starting early re-emits records the Job already wrote. That is deliberate:
a duplicate is recoverable, a gap is a lost record. The Kafka adapter is
at-least-once too. Before the switch, confirm every
Downstream System tolerates a repeated record for the same key.

After the switch, compare output and DLQ volumes, and Downstream System
results such as warranty flags, with the shadow period.

## 3. Roll back

Stop the Processor and restart the Job from its last checkpoint. This
replays input the Processor already handled, so again expect duplicates, not
gaps. Downstream Systems never changed, so nothing else moves. Keep the Job
deployable until the rollback window closes.

## 4. Retire

When the rollback window closes, remove the Job's deployment. Keep
`jobs/<name>/` and its Goldens in the repository. The Job is the
specification, and every later change to the Processor is Gated against it.

## Out of scope

- The rollback window's length and who signs off the switch are team
  decisions.
- Exactly-once delivery. The Kafka adapter in
  `processors/telemetry-core/src/kafka.rs` is at-least-once by design.
