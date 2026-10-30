# telemetry-migration

Legacy Scala/Spark Jobs under `jobs/` are being replaced one at a time by Rust Processors under `processors/`. Every replacement must pass the migration Gate. Vocabulary is in `CONTEXT.md`; use it.

## Commands

- `make gate PROCESSOR=<name>` - run the full Gate for one Processor and write its Evidence Pack to `evidence/<name>/<sha>/`.
- The `3. Gate verdict` check on a PR sums up `make gate` rerun in GitHub Actions on the PR head commit. It is the verdict that counts. A local `make gate` is a preview of it.
- `make goldens JOB=<name>` - regenerate Goldens from the Scala Job. Platform team only.
- `make test`, `make fmt`, `make lint` - all three languages.
- `make setup`, `make doctor` - install and verify the macOS toolchain.
- Rust: `cargo build` never needs Kafka. `--features kafka` is optional and not required for any test.

## README is the setup guide

`README.md` is written for a maintainer who does not know Scala, Rust or Spark and runs everything on macOS. If your change adds or changes a tool, dependency, environment variable, Makefile target or first-run step, update `README.md` "Local setup (macOS)" or "Running" and extend `make doctor` in the same PR. Do not assume the reader knows what is obvious to a Rust or Scala developer.

Exception: a Processor may reuse a Rust crate version already in
`processors/Cargo.lock` without README or doctor edits when the lockfile,
toolchain, system dependencies and setup steps stay unchanged. This includes
moving a dev-dependency to a runtime dependency or enabling crate features
within those limits. Edit only that Processor's own `Cargo.toml`, not the
shared workspace manifest, and explain the reuse in the PR. If a Migration
needs a setup change outside its per-Processor scope, stop and request
platform preparation instead.

## Gate inputs are read-only

These paths are the inputs to the Gate. They are owned by the platform and privacy teams and must not be modified by a Migration:

- `goldens/`
- `fixtures/`
- `contracts/`
- `harness/`
- `jobs/`
- `manifests/*.job.yaml`
- `manifests/classification.yaml`
- `downstream/`
- `.github/`
- `schemas/`

If the Gate is red, the fix goes in `processors/<name>/`. If you believe a Golden, Manifest, contract or Harness check is itself wrong, stop, explain why in your report with file and line references, and do not change it.

`manifests/<name>.processor.yaml` is yours to update so it describes what the Processor actually reads, derives, emits and logs. Never loosen a classification, add an output field, add a destination or add a purpose that the matching `*.job.yaml` Baseline does not have.

## Source of truth

The Scala Job is the specification. When the Gate reports a Drift, read the Job before changing the Processor. Do not infer behaviour from field names or from what seems reasonable.
