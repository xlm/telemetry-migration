---
name: migrate-job
description: Migrate one legacy Scala/Spark Job under jobs/ to its Rust Processor under processors/, through the migration Gate, and open a PR carrying the Evidence Pack. Use when asked to migrate, port or re-implement a Job.
argument-hint: <job-name> [target-branch]
triggers: ["user"]
---

# Migrate Job `$1`

The Job is `jobs/$1/`. Its Processor scaffold is `processors/$1/`. Its Baseline is `manifests/$1.job.yaml`. Read `CONTEXT.md` and `AGENTS.md` before anything else.

Resolve the arguments before running commands. `$1` is the Job name. `TARGET_BRANCH` is the second argument when supplied, otherwise the literal `main`. Substitute that resolved branch name for `<TARGET_BRANCH>` below, never an empty `$2` or an unset shell variable. Fetch `origin/<TARGET_BRANCH>` and create your own working branch from it. Never push to the target branch. If the Processor is already implemented on that base, report instead of replacing it with a scaffold. For the live demo, resolve `TARGET_BRANCH` to `demo/live-migration` explicitly.

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

Cite the Scala file and line for each rule. Mark rules that do not apply to this Job as such. If a rule cannot be determined from the code, stop rather than guessing.

## 2. Check the Baseline

Compare the characterisation against `manifests/$1.job.yaml`. If the Scala reads, emits or logs a field the Baseline does not declare, stop and report. Do not proceed.

## 3. Implement

Implement `processors/$1/src/core.rs` as a pure core, following the module layout of `processors/ota-status/` (`types.rs`, `core.rs`, `main.rs`). Unit tests for each rule in the characterisation, especially dedup, out-of-order, late, timeout and numeric rounding.

## 4. Describe

Update `manifests/$1.processor.yaml` to describe exactly what the Processor reads, derives, emits and logs. Do not add fields, destinations or purposes absent from the Baseline.

## 5. Gate

Run `cargo test --manifest-path processors/Cargo.toml -p $1`, `make fmt` and `make lint`, then commit the implementation and characterisation. Run `make gate PROCESSOR=$1 BASE=origin/<TARGET_BRANCH>` from the clean commit so the Evidence Pack identifies the tested code. For example, an omitted target for charging-session gives `make gate PROCESSOR=charging-session BASE=origin/main`. The demo gives `make gate PROCESSOR=charging-session BASE=origin/demo/live-migration`. For every Drift, go back to the Scala, find what it actually does, fix the Rust, commit and re-run. Never touch `goldens/`, `fixtures/`, `contracts/`, `harness/`, `jobs/`, `downstream/`, `.github/`, `schemas/`, `manifests/classification.yaml` or the Baseline.

## 6. PR

When the Gate is green, open a PR against the resolved `TARGET_BRANCH`. The description must contain, in this order:

1. The Evidence Pack Markdown from `evidence/$1/<sha>/evidence.md`, verbatim.
2. Every Drift found during step 5, with its root cause in the Scala.
3. The known limitations and ambiguities from `CHARACTERISATION.md`.

## 7. Gate check

Wait for the `3. Gate verdict` check on the PR. It sums up the Gate rerun in GitHub Actions on the PR head commit, and it is the verdict that counts. If it is red, read the log of each red `2. Gate (<name>)` job:

- The log contains a Gate report (a `Gate: RED - <steps>` line, followed by the later workflow steps): treat every finding as in step 5. Find what the Scala does, fix the Rust, push, and wait for the check again.
- The log has no Gate report (runner fault, failed download, timeout, cancelled run): it is an infrastructure failure, not a fix attempt. Re-run the failed jobs once. If the re-run also has no Gate report, or you can't re-run, report it.

After every push, replace the Evidence Pack in the PR description with the one from your latest `make gate` run.

Keep the Evidence Pack in the PR description, not a new commit. Do not merge the PR or change the target branch. A green Gate is readiness for human review, not approval.

## Stop conditions

Report instead of continuing when:

- step 2 finds undeclared data handling
- a rule in the Scala cannot be determined from the code
- the Gate is still red after three fix attempts on the same Drift, counting local runs and the `3. Gate verdict` check together
- any fix would require editing a read-only path

## Acceptance

- `3. Gate verdict` check green on the PR.
- Diff confined to `processors/$1/` and `manifests/$1.processor.yaml`.
- PR description structured as in step 6.
