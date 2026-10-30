# telemetry-migration

This repository demonstrates replacing legacy Scala/Spark Jobs with pure-core Rust Processors. A Harness compares Processor output with Goldens and checks contracts and privacy declarations before producing a reviewable Evidence Pack.

## Local setup (macOS)

Homebrew is the only preinstalled package manager assumed here. Rust also
requires Apple's Command Line Tools.

Install the compiler tools:

```sh
xcode-select --install
```

Wait for the Command Line Tools installer to finish. Success cue: this prints
the installed tools directory:

```sh
xcode-select -p
```

Clone the repository and enter its directory:

```sh
git clone https://github.com/xlm/telemetry-migration.git
cd telemetry-migration
git checkout main
```

Success cue: `git status --short --branch` reports the `main` branch.

Install JDK 17, sbt, and uv:

```sh
brew install openjdk@17 sbt uv
brew list --versions openjdk@17 sbt uv
```

Success cue: the command lists installed versions for all three packages.

Install rustup, then the Rust version pinned in `rust-toolchain.toml` with
its formatting and lint components. Run this from the repository root:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
. "$HOME/.cargo/env"
rustup toolchain install
rustup show active-toolchain
```

Success cue: `rustup show active-toolchain` starts with `1.99.1` and says it
is overridden by `rust-toolchain.toml`.

Install uv's managed Python 3.12:

```sh
uv python install 3.12
uv --version
uv run --project harness --python 3.12 python --version
```

Success cue: the Python command prints `Python 3.12.x`.

Homebrew's OpenJDK formula is keg-only. Register it before exporting
`JAVA_HOME` or running `make setup`:

```sh
sudo mkdir -p /Library/Java/JavaVirtualMachines
sudo ln -sfn "$(brew --prefix openjdk@17)/libexec/openjdk.jdk" /Library/Java/JavaVirtualMachines/openjdk-17.jdk
ls -ld /Library/Java/JavaVirtualMachines/openjdk-17.jdk
```

Success cue: `ls` shows the `openjdk-17.jdk` symbolic link.

Select JDK 17 for this terminal and verify the version:

```sh
export JAVA_HOME=$(/usr/libexec/java_home -v 17)
"$JAVA_HOME/bin/java" -version
```

Success cue: Java reports version 17.

Set Spark's local address and install and verify the project:

```sh
export SPARK_LOCAL_IP=127.0.0.1
make setup
make doctor
```

Success cue: the final line from `make doctor` is `doctor: ok`.

### Toolchain and dependencies

| Component | Version | Purpose |
| --- | --- | --- |
| JDK | 17 | Scala and Spark runtime |
| sbt | 1.10.7 | Scala build |
| Scala | 2.12.20 | Spark-compatible Job build |
| Spark | 3.5.5 | Job execution in `local[2]` mode |
| Rust | 1.99.1 (`rust-toolchain.toml`) with rustfmt and clippy | Processor workspace |
| Python | 3.12 | Harness and fixture generator |
| uv | Current stable with PEP 735 dependency-group support | Python environment and commands |

The default Rust build needs only rustup/cargo and the Apple Command Line
Tools. It does not need Kafka, cmake, or librdkafka. The Command Line Tools
include the C++ compiler needed by the optional Kafka adapter. To build it,
install cmake:

```sh
brew install cmake
cargo build --manifest-path processors/Cargo.toml -p telemetry-core --features kafka
```

The first Kafka-feature build compiles librdkafka from source and takes a few
minutes. A broker is needed only to run the adapter, and is available through
`deploy/docker-compose.yaml`.

Python runtime dependencies are Click, jsonschema, and PyYAML. The `dev`
dependency group contains pytest and Ruff. Setuptools is the package build
backend. `uv sync` creates the project-local `harness/.venv`; no manually
managed or global virtual environment is needed, and Make targets use `uv run`.

The Scala build uses the JDK 17 `--add-opens` options required by Spark. Those
flags live in `jobs/build.sbt`, so there is nothing to add by hand. Makefile
targets set `SPARK_LOCAL_IP=127.0.0.1` for Spark commands, so the export above
is optional when using them. In one clean-cache measurement, sbt fetched about
243 MB and took 27 seconds here. First-run times vary by network. Homebrew and
Rust provide native Apple Silicon tools on Apple Silicon Macs. Official
references: [Homebrew](https://brew.sh/),
[OpenJDK 17](https://formulae.brew.sh/formula/openjdk@17),
[sbt](https://www.scala-sbt.org/1.x/docs/Installing-sbt-on-Mac.html),
[uv](https://docs.astral.sh/uv/getting-started/installation/),
[rustup](https://rustup.rs/),
[Scala 2.12.20](https://www.scala-lang.org/download/2.12.20.html), and
[Spark 3.5.5](https://spark.apache.org/docs/3.5.5/).

### Linux bootstrap prerequisites

On Ubuntu, install `make` and `curl`, and have `sudo` available for apt before
running `make setup`. The bootstrap uses apt to install JDK 17 and the C/C++
compiler toolchain. Other supported Linux package managers are dnf, yum, and
pacman.

For the optional Kafka adapter, install cmake with `sudo apt-get install cmake`.

sbt falls back to Google's Maven Central mirror if Maven Central rate-limits.

## Running

Run the repository checks:

```sh
make test
make fmt
make lint
```

The fixture generator command is `make fixtures`. To demonstrate the current
Job and Processor names, these are the Goldens and Gate commands:

```sh
make goldens JOB=ota-status
make gate PROCESSOR=ota-status
make goldens JOB=battery-health
make gate PROCESSOR=battery-health
```

`make goldens JOB=ota-status`, `make goldens JOB=charging-session` and
`make goldens JOB=battery-health` run the
Job over every `.jsonl` file in `fixtures/<job>/` and rewrite
`goldens/<job>/`. The generated files are byte-identical across runs when the
code and fixtures are unchanged. Never edit Goldens by hand. With dependencies
cached, the runs took 15 seconds for ota-status and 20 seconds for
charging-session here. First runs may take longer.

`battery-health` is a small stateless Job that clamps state-of-health to
0-100 and classifies it at 60 and 80 percent. All three Rust Processor
cores are intentionally unfinished migration exercises.
`make doctor` verifies every registered Processor has its contract,
Baseline and Rust crate, and compiles all three Jobs and Processors.

### Live migration demo

Use `main` as the starting branch and PR target. Start a Devin there to
migrate `ota-status`, then review and show the reusable
[`migrate-job` skill](.agents/skills/migrate-job/SKILL.md). During the live
demo, a human may merge OTA after its green Gate and review. Start a
supervising Devin from the updated `main` and launch exactly two children:

- `charging-session`: follow the skill normally and produce a green PR.
- `battery-health`: use the skill for characterisation, implementation and
  tests, with an explicit demo-only exception for a synthetic negative
  test. Deliberately log a raw fixture `vehicle_id` in the Processor's
  stderr. Require Harness, Contract and Path check green, Downstream
  System skipped, and Privacy red with `raw-vehicle-id-in-log`. Publish
  the red Evidence Pack in a clearly labelled, do-not-merge PR and stop.
  Do not remediate this intentional failure or change the normal skill.

Each child starts from the same recorded `main` commit, owns its own
branch, and changes only its Processor directory and Processor Manifest.
Every migration PR targets `main`, with `BASE=origin/main` for the Gate.
The skill already defaults to `main`. Never push directly to `main`.
The supervisor reports charging's green verdict and battery's expected
privacy-blocked verdict separately, not overall migration success.
An unrelated failure or a scaffold crash does not count as the negative
test. All data used for it must be synthetic fixture data.

For practice, merge no PRs. During the live demo, a human may merge OTA
and charging after review and green Gates. Battery must remain unmerged.
Before the first run, tag the prepared `main` commit with all three cores
unfinished. After live merges, repeating the demo requires restoring that
tagged starting point. A force reset requires explicit human approval,
branch rules that permit it, no active sessions, a check for unrelated
work, and `--force-with-lease`. Never weaken branch protection. Resetting
the branch does not erase merged PRs or CI history. Agents must not merge,
tag, or reset automatically. The old `demo/live-migration` and
`demo/charging-session-migrated` branches are not used by this storyline.

The scaffold-preparation PR intentionally fails OTA's Harness check and
the existing OTA integration test in `make test`. Keep that test intact
for the live migration to satisfy. These failures are not the
privacy-negative example and not a completed Migration.
Do not bypass the Gate to land preparation. It needs an approved
repository preparation process before `main` becomes the starting point.

Run Gate for one Processor. `BASE` changes the comparison base for the Path
check, and `BIN` supplies a Processor binary and skips the Cargo build:

```sh
make gate PROCESSOR=ota-status
make gate PROCESSOR=charging-session BASE=origin/main BIN=processors/target/release/charging-session
```

### Gate on PRs

Every pull request runs the Gate in GitHub Actions
(`.github/workflows/gate.yml`), in three numbered jobs:

1. `1. Select Processors` runs `gate-processors`, which lists the Processors
   the PR's changes can affect. A Processor's own crate or Processor Manifest
   selects that Processor. A Gate input, the `Makefile`, `rust-toolchain.toml`
   or shared Rust selects every Processor. Anything else selects none.
2. `2. Gate (<name>)`, one per selected Processor, runs `make gate` on the PR
   head commit against the PR's base branch. It shows the Evidence Pack on the
   job summary page and uploads it with the raw Processor output as the
   `gate-<name>` artifact.
3. `3. Gate verdict` is green when every selected Processor's Gate is green or
   none was needed. Its summary table explains each result with the Evidence
   Pack headline in the Why column. It is the one check `main` requires,
   because the number of `2. Gate` jobs changes from PR to PR.

A push to `main` only builds the Processors, so later PRs start with a warm
build cache. The cache is keyed on the Rust version, so after bumping
`rust-toolchain.toml`, warm it by hand: Actions, Gate, Run workflow on `main`.

The Gate runs the PR's own `harness/` and workflow, so a PR could weaken
the Gate and still pass it. `.github/CODEOWNERS` covers every Gate input,
the `Makefile` and `rust-toolchain.toml`. It also covers the agent's rules
(`AGENTS.md`, `CONTEXT.md`, `.agents/`). The Gate doesn't read those, but
they hold the stop conditions a Migration must follow. With "Require review
from Code Owners" set on `main`, GitHub enforces that review from the base
branch, where the PR can't change it.

To see which Processors a branch would Gate:

```sh
uv run --project harness --python 3.12 gate-processors --base origin/main
```

### Reading a red Gate

Gate runs all six steps even after a failure, so the report includes Harness,
Contract, Downstream System, Privacy, Path check, and Evidence Pack results.
It finishes with `Gate: RED - <steps>` and exit code 1 if any step is red.
That final line names the red steps. A red Harness means the Processor could
not run or its output has Drift from the Golden. A red Contract means a
Processor record violates the output schema or a semantic assertion. A red
Downstream System means its warranty flags differ from the Golden or the
Downstream System script crashed. A red Privacy step means the Processor
Manifest or runtime output/logs violate the Baseline policy. A red Path check
means the branch touched a Gate input that a Migration must leave unchanged.
A red Evidence Pack means the report could not be written. A skipped
Downstream System means that Processor's output has no warranty destination.

The Evidence Pack is written to
`evidence/<processor>/<short-sha>/evidence.md` and
`evidence/<processor>/<short-sha>/evidence.json`. Raw Processor output,
DLQ records, and logs, including `<fixture>.stdout.log` and
`<fixture>.stderr.log`, are under `target/gate/<processor>/`.
The pack opens with a headline saying whether it is ready or not ready for
human review, a five-step strip, what changed, what failed, and residual
uncertainties. The full technical record stays in a closed Reviewer detail.
The `summary` in `evidence.json` is its machine-readable twin.

A Harness finding such as
`changed CS-00000001 event_count: Golden 4, Processor 5` reports Drift:
Golden is what the Job produced, and Processor is the candidate output. A
`missing CS-00000001` line means the Golden record is absent from Processor
output. A `dlq missing (event-id, REASON) x1` line means the Golden had one
such Dead-Letter Queue entry that the Processor did not emit.

A Privacy finding can read
`field end_location, classification sensitive-location, not permitted on topic vehicle.charging.sessions, Baseline unchanged`
with disposition `policy violation - requires remediation`. The Evidence
Pack includes the finding and disposition for reviewer assessment. The Gate
does not stop early, and all six step results remain available even when
multiple steps are red.

On a PR, start at the `3. Gate verdict` check: its summary page lists each
Processor's final Gate line. A red `2. Gate (<name>)` job has the full report
in its log, and the Evidence Pack on its summary page and in its
`gate-<name>` artifact. A job whose log has no Gate report failed outside the
Gate, for example a runner fault, a failed download or a timeout. Re-run it
before changing any code.

### Rust processors

Build the release binaries from the repository root:

```sh
cargo build --release --manifest-path processors/Cargo.toml --workspace
```

The binaries are written to `processors/target/release/<name>`. Each binary
accepts `--input F --output O --dlq D --log L`, for example:

```sh
processors/target/release/ota-status \
  --input fixtures/ota-status/02-variant-status-strings.jsonl \
  --output /tmp/ota-output.jsonl \
  --dlq /tmp/ota-dlq.jsonl \
  --log /tmp/ota-log.jsonl
```

The output and DLQ files contain JSONL records. The log file contains JSON
log lines, with `vehicle_id` represented only by its 12-character hash.
All three Processor cores are scaffolds and panic with `not yet
implemented` until their Jobs are migrated.

## Repository map

- `jobs/` contains the legacy Scala/Spark Job build.
- `processors/` contains the Rust workspace, shared telemetry types, adapters, and Processor binaries.
- `harness/` contains the Python Harness package and deterministic fixture generator.
- `schemas/`, `contracts/`, `manifests/`, and `fixtures/` define the input, output, data-handling, and test records.
- `goldens/` contains the Job output for each fixture, generated by `make goldens`. Each Processor is compared against it.
- `downstream/` contains the Downstream System scripts the Gate runs against Processor output.
- `evidence/` contains generated Evidence Packs at `<processor>/<short-sha>/`; do not commit them during a Migration.
- `.github/` contains the Gate workflow (`workflows/gate.yml`) and `CODEOWNERS`.
- `.agents/skills/` contains the `migrate-job` skill an agent follows for a Migration.
- `rust-toolchain.toml` pins the Rust version for CI and local builds.
- `docs/` and `deploy/` are placeholders for later phases.
