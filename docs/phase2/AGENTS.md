# telemetry-migration

Legacy Scala/Spark Jobs under `jobs/` are being replaced one at a time by Rust Processors under `processors/`. Every replacement must pass the migration Gate. Vocabulary is in `CONTEXT.md`; use it.

## Commands

- `make gate PROCESSOR=<name>` - run the full Gate for one Processor and write its Evidence Pack to `evidence/<name>/<sha>/`.
- `make goldens JOB=<name>` - regenerate Goldens from the Scala Job. Platform team only.
- `make test`, `make fmt`, `make lint` - all three languages.
- Rust: `cargo build` never needs Kafka. `--features kafka` is optional and not required for any test.

## Gate inputs are read-only

These paths are the inputs to the Gate. They are owned by the platform and privacy teams and must not be modified by a Migration:

- `goldens/`
- `fixtures/`
- `contracts/`
- `harness/`
- `jobs/`
- `manifests/*.job.yaml`
- `downstream/`

If the Gate is red, the fix goes in `processors/<name>/`. If you believe a Golden, Manifest, contract or Harness check is itself wrong, stop, explain why in your report with file and line references, and do not change it.

`manifests/<name>.processor.yaml` is yours to update so it describes what the Processor actually reads, derives, emits and logs. Never loosen a classification, add an output field, add a destination or add a purpose that the matching `*.job.yaml` Baseline does not have.

## Source of truth

The Scala Job is the specification. When the Gate reports a Drift, read the Job before changing the Processor. Do not infer behaviour from field names or from what seems reasonable.
