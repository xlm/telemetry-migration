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

Install Rust stable with its formatting and lint components:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
. "$HOME/.cargo/env"
rustup toolchain install stable
rustup default stable
rustup component add rustfmt clippy --toolchain stable
rustup show active-toolchain
```

Success cue: `rustup show active-toolchain` starts with `stable`.

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
| Rust | stable with rustfmt and clippy | Processor workspace |
| Python | 3.12 | Harness and fixture generator |
| uv | Current stable with PEP 735 dependency-group support | Python environment and commands |

Python runtime dependencies are Click, jsonschema, and PyYAML. The `dev`
dependency group contains pytest and Ruff. Setuptools is the package build
backend. `uv sync` creates the project-local `harness/.venv`; no manually
managed or global virtual environment is needed, and Make targets use `uv run`.

The Scala build uses the JDK 17 `--add-opens` options required by Spark. The
first sbt run downloads Spark and Scala dependencies and may take several
minutes. Homebrew and Rust provide native Apple Silicon tools on Apple Silicon
Macs. Official references: [Homebrew](https://brew.sh/),
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
```

Goldens and Gate intentionally exit with status 1 and a not-implemented
message. This scaffold does not yet implement either command, Processor
business logic, Goldens, or an Evidence Pack. No `evidence/` directory or
committed Evidence Pack exemplar currently exists, and these stub commands
write no output.

When Gate is implemented, a red result will report findings from every step,
including fixture-level Harness counts, contract failures, Downstream System
flag differences, privacy dispositions, and path-check failures. A nonzero
status means at least one Gate step is red. Read the summary and findings in
the terminal, then review the generated Evidence Pack at
`evidence/ota-status/<short-sha>/evidence.md` and
`evidence/ota-status/<short-sha>/evidence.json`. The fixed residual
uncertainties will identify fixture coverage, value-based runtime scanning,
and untested broker semantics.

The `charging-session.processor.yaml` scaffold is intentionally byte-identical
to its Job Baseline, including `kind: job`. A future Migration changes its
kind to `processor`.

## Repository map

- `jobs/` contains the legacy Scala/Spark Job build.
- `processors/` contains the Rust workspace, shared telemetry types, adapters, and Processor binaries.
- `harness/` contains the Python Harness package and deterministic fixture generator.
- `schemas/`, `contracts/`, `manifests/`, and `fixtures/` define the input, output, data-handling, and test records.
- `goldens/` is reserved for outputs generated by running the Jobs.
- `evidence/` is reserved for generated review packs and is absent until Gate is implemented.
