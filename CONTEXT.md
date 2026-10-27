# Telemetry Migration Demo

A synthetic vehicle-telemetry platform in which legacy Scala/Spark jobs are replaced, one at a time, by Rust processors, with each replacement producing reviewable evidence. Built to demonstrate Devin to Lucid Motors engineering leadership.

## Language

**Job**:
A legacy Scala/Spark Streaming unit of work that reads one or more Kafka input topics and writes an output topic. The thing being retired.
_Avoid_: Pipeline, stage, stream, legacy service

**Processor**:
The Rust replacement for exactly one Job. Reads the same input topics and must write the same output contract.
_Avoid_: Service, worker, port, new job

**Migration**:
The replacement of one Job by one Processor, together with the evidence that the replacement is safe.
_Avoid_: Port, rewrite, conversion, cutover (cutover is one step within a Migration)

**Downstream System**:
An application (service, warranty, OTA) that reads a Job's or Processor's output topic and must keep working unchanged through a Migration.
_Avoid_: Consumer (collides with Kafka consumer group), client, subscriber

**Evidence Pack**:
The generated per-Migration artefact that a human privacy or engineering reviewer reads to decide whether the Migration stays within the approved Baseline.
_Avoid_: Privacy review (that is the human act, which the pack informs but does not replace), report, audit

## Processing

**Vehicle Clock**:
The highest event time seen so far for one vehicle, heartbeats included. The only clock a Job or Processor may consult to decide that a session is idle or an event is late.
_Avoid_: Watermark (Spark-specific), wall clock, processing time, now

**Heartbeat**:
A vehicle event that carries no session and exists only to advance that vehicle's Vehicle Clock. Never emitted, never counted, never rejected.
_Avoid_: Keepalive, ping, tick

## Verification

**Golden**:
The expected output for a fixture, produced by running the Job itself. Never hand-authored.
_Avoid_: Expected output, snapshot, oracle

**Harness**:
The differential runner that feeds the same fixtures to a Job's Goldens and a Processor and compares the outputs per key.
_Avoid_: Test suite, comparator, diff tool

**Drift**:
A behavioural difference between a Processor and its Job that the Harness detects. Distinct from a schema break (the output is still valid, the values or timing differ).
_Avoid_: Bug, regression, incompatibility, mismatch

**Gate**:
The aggregate verdict over Harness, contract checks and privacy checks that decides whether a Migration may proceed to human review. The verdict that counts is the one recorded on the Migration's PR. A run anywhere else is a preview of it.
_Avoid_: CI, pipeline, approval (approval is the human step after a green Gate)

## Data handling

**Manifest**:
The machine-readable declaration of what data a Job or Processor reads, derives, emits and logs, with each field's classification and allowed destinations.
_Avoid_: Policy, schema, config

**Baseline**:
The approved Manifest of a Job, against which its Processor's Manifest and runtime behaviour are compared.
_Avoid_: Reference, legacy manifest, source of truth
