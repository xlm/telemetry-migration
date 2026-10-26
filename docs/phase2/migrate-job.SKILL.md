---
name: migrate-job
description: Migrate one legacy Scala/Spark Job under jobs/ to its Rust Processor under processors/, through the migration Gate, and open a PR carrying the Evidence Pack. Use when asked to migrate, port or re-implement a Job.
argument-hint: <job-name>
triggers: ["user"]
---

# Migrate Job `$1`

The Job is `jobs/$1/`. Its Processor scaffold is `processors/$1/`. Its Baseline is `manifests/$1.job.yaml`. Read `CONTEXT.md` and `AGENTS.md` before anything else.

## 1. Characterise

Read `jobs/$1/` completely, including util and config files. Write `processors/$1/CHARACTERISATION.md` listing every business rule you find:

- validation and each DLQ reason
- dedup key and tie-break
- state key
- close conditions
- timeout semantics: event time or processing time, and what timestamp the output carries
- aggregation formulas, including rounding mode and precision
- output fields and how each is derived
- what is logged and how identifiers are treated

Cite the Scala file and line for each rule. If a rule is ambiguous, record the ambiguity and how you resolved it.

## 2. Check the Baseline

Compare the characterisation against `manifests/$1.job.yaml`. If the Scala reads, emits or logs a field the Baseline does not declare, stop and report. Do not proceed.

## 3. Implement

Implement `processors/$1/src/core.rs` as a pure core, following the module layout of `processors/ota-status/` (`types.rs`, `core.rs`, `main.rs`). Unit tests for each rule in the characterisation, especially dedup, out-of-order, late, timeout and numeric rounding.

## 4. Describe

Update `manifests/$1.processor.yaml` to describe exactly what the Processor reads, derives, emits and logs. Do not add fields, destinations or purposes absent from the Baseline.

## 5. Gate

Run `make gate PROCESSOR=$1`. For every Drift, go back to the Scala, find what it actually does, fix the Rust, re-run. Never touch `goldens/`, `fixtures/`, `contracts/`, `harness/`, `jobs/`, `downstream/` or the Baseline.

## 6. PR

When the Gate is green, open a PR against `main`. The description must contain, in this order:

1. The Evidence Pack Markdown from `evidence/$1/<sha>/evidence.md`, verbatim.
2. Every Drift found during step 5, with its root cause in the Scala.
3. The known limitations and ambiguities from `CHARACTERISATION.md`.

## Stop conditions

Report instead of continuing when:

- step 2 finds undeclared data handling
- a rule in the Scala cannot be determined from the code
- the Gate is still red after three fix attempts on the same Drift
- any fix would require editing a read-only path

## Acceptance

- `make gate PROCESSOR=$1` green.
- Diff confined to `processors/$1/` and `manifests/$1.processor.yaml`.
- PR description structured as in step 6.
