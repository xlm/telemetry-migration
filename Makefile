SHELL := /bin/sh

PYTHON_VERSION := 3.12
SBT_VERSION := 1.10.7
SBT_REPOSITORY_FLAGS := -Dsbt.override.build.repos=true -Dsbt.repository.config=project/repositories -Dsbt.supershell=false

export PATH := $(HOME)/.local/bin:$(HOME)/.cargo/bin:$(PATH)

.PHONY: setup doctor gate goldens fixtures test fmt lint

setup:
	@set -eu; \
	os=$$(uname -s); \
	case "$$os" in \
		Darwin) \
			if ! command -v brew >/dev/null 2>&1; then \
				printf '%s\n' 'Homebrew is required. Install it from https://brew.sh/.'; \
				exit 1; \
			fi; \
			export PATH="$$HOME/.local/bin:$$HOME/.cargo/bin:$$PATH"; \
			for package in openjdk@17 sbt uv; do \
				if brew list --versions "$$package" >/dev/null 2>&1; then \
					brew upgrade "$$package"; \
				else \
					brew install "$$package"; \
				fi; \
			done; \
			if ! command -v rustup >/dev/null 2>&1; then \
				curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable; \
			fi; \
			export PATH="$$HOME/.local/bin:$$HOME/.cargo/bin:$$PATH"; \
			rustup update stable; \
			rustup default stable; \
			rustup component add rustfmt clippy --toolchain stable; \
			jdk_source=$$(brew --prefix openjdk@17)/libexec/openjdk.jdk; \
			jdk_parent=/Library/Java/JavaVirtualMachines; \
			jdk_target=$$jdk_parent/openjdk-17.jdk; \
			if [ ! -d "$$jdk_source" ]; then \
				printf 'Homebrew JDK not found at %s\n' "$$jdk_source"; \
				exit 1; \
			elif [ -L "$$jdk_target" ] && [ "$$(readlink "$$jdk_target")" = "$$jdk_source" ]; then \
				:; \
			elif [ -L "$$jdk_target" ] || [ -e "$$jdk_target" ]; then \
				printf 'Refusing to replace existing JDK path: %s\n' "$$jdk_target"; \
				printf 'Remove or correct it, then rerun make setup.\n'; \
				exit 1; \
			elif [ -w "$$jdk_parent" ]; then \
				ln -s "$$jdk_source" "$$jdk_target"; \
			else \
				printf '%s\n' 'Register the Homebrew JDK with these commands, then rerun make setup:'; \
				printf 'sudo mkdir -p %s\n' "$$jdk_parent"; \
				printf 'sudo ln -s "%s" "%s"\n' "$$jdk_source" "$$jdk_target"; \
				exit 1; \
			fi; \
			JAVA_HOME=$$(/usr/libexec/java_home -v 17); \
			export JAVA_HOME; \
			;; \
		Linux) \
			run_root() { \
				if [ "$$(id -u)" -eq 0 ]; then \
					"$$@"; \
				elif command -v sudo >/dev/null 2>&1; then \
					sudo "$$@"; \
				else \
					printf '%s\n' 'Install JDK 17, curl, tar, and a C/C++ compiler with your Linux package manager, then rerun make setup.'; \
					return 1; \
				fi; \
			}; \
			if command -v apt-get >/dev/null 2>&1; then \
				run_root apt-get update; \
				run_root apt-get install -y openjdk-17-jdk curl ca-certificates tar build-essential; \
			elif command -v dnf >/dev/null 2>&1; then \
				run_root dnf install -y java-17-openjdk-devel curl ca-certificates tar gcc gcc-c++ make; \
			elif command -v yum >/dev/null 2>&1; then \
				run_root yum install -y java-17-openjdk-devel curl ca-certificates tar gcc gcc-c++ make; \
			elif command -v pacman >/dev/null 2>&1; then \
				run_root pacman -Sy --needed --noconfirm jdk17-openjdk curl ca-certificates tar base-devel; \
			else \
				printf '%s\n' 'No supported Linux package manager found. Install JDK 17, curl, tar, and a C/C++ compiler, then rerun make setup.'; \
				exit 1; \
			fi; \
			export PATH="$$HOME/.local/bin:$$HOME/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:$$PATH"; \
			if ! command -v uv >/dev/null 2>&1; then \
				curl -LsSf https://astral.sh/uv/install.sh | sh; \
			fi; \
			if ! command -v rustup >/dev/null 2>&1; then \
				curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable; \
			fi; \
			export PATH="$$HOME/.local/bin:$$HOME/.cargo/bin:$$PATH"; \
			rustup update stable; \
			rustup default stable; \
			rustup component add rustfmt clippy --toolchain stable; \
			mkdir -p "$$HOME/.local/opt" "$$HOME/.local/bin"; \
			if [ ! -x "$$HOME/.local/opt/sbt/bin/sbt" ] && [ ! -x "$$HOME/.local/opt/sbt-$(SBT_VERSION)/bin/sbt" ]; then \
				sbt_archive=$$(mktemp); \
				curl -fL --retry 3 "https://github.com/sbt/sbt/releases/download/v$(SBT_VERSION)/sbt-$(SBT_VERSION).tgz" -o "$$sbt_archive"; \
				tar -xzf "$$sbt_archive" -C "$$HOME/.local/opt"; \
				rm -f "$$sbt_archive"; \
			fi; \
			if [ -x "$$HOME/.local/opt/sbt/bin/sbt" ]; then \
				ln -sfn "$$HOME/.local/opt/sbt/bin/sbt" "$$HOME/.local/bin/sbt"; \
			elif [ -x "$$HOME/.local/opt/sbt-$(SBT_VERSION)/bin/sbt" ]; then \
				ln -sfn "$$HOME/.local/opt/sbt-$(SBT_VERSION)/bin/sbt" "$$HOME/.local/bin/sbt"; \
			else \
				printf '%s\n' 'The pinned sbt archive did not contain its launcher script.'; \
				exit 1; \
			fi; \
			java_home=$$(find /usr/lib/jvm -mindepth 1 -maxdepth 1 -type d -name '*17*' -print 2>/dev/null | sort | head -n 1); \
			if [ -z "$$java_home" ] || [ ! -x "$$java_home/bin/java" ]; then \
				printf '%s\n' 'Could not find JDK 17 under /usr/lib/jvm after installation.'; \
				exit 1; \
			fi; \
			JAVA_HOME=$$java_home; \
			export JAVA_HOME; \
			;; \
		*) \
			printf 'Unsupported operating system: %s\n' "$$os"; \
			exit 1; \
			;; \
	esac; \
	export PATH="$$HOME/.local/bin:$$HOME/.cargo/bin:$$PATH"; \
	uv python install $(PYTHON_VERSION); \
	uv sync --project harness --python $(PYTHON_VERSION); \
	cargo build --manifest-path processors/Cargo.toml --workspace; \
	export SPARK_LOCAL_IP=127.0.0.1; \
	sbt_opts="$${SBT_OPTS:-} $(SBT_REPOSITORY_FLAGS)"; \
	cd jobs && SBT_OPTS="$$sbt_opts" sbt -batch compile

doctor:
	@set -eu; \
	export PATH="$$HOME/.local/bin:$$HOME/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:$$PATH"; \
	os=$$(uname -s); \
	case "$$os" in \
		Darwin) \
			if [ ! -x /usr/libexec/java_home ]; then \
				printf '%s\n' 'macOS Java discovery tool /usr/libexec/java_home is unavailable.'; \
				exit 1; \
			fi; \
			JAVA_HOME=$$(/usr/libexec/java_home -v 17); \
			;; \
		Linux) \
			JAVA_HOME=$$(find /usr/lib/jvm -mindepth 1 -maxdepth 1 -type d -name '*17*' -print 2>/dev/null | sort | head -n 1); \
			;; \
		*) \
			printf 'Unsupported operating system: %s\n' "$$os"; \
			exit 1; \
			;; \
	esac; \
	if [ -z "$$JAVA_HOME" ] || [ ! -x "$$JAVA_HOME/bin/java" ]; then \
		printf '%s\n' 'JAVA_HOME must resolve to a JDK 17 installation.'; \
		exit 1; \
	fi; \
	export JAVA_HOME; \
	java_version=$$("$${JAVA_HOME}/bin/java" -version 2>&1 | head -n 1); \
	printf 'JAVA_HOME=%s (%s)\n' "$$JAVA_HOME" "$$java_version"; \
	case "$$java_version" in *'"17.'*) ;; *) printf '%s\n' 'JDK 17 is required.'; exit 1 ;; esac; \
	uv --version; \
	python_version=$$(uv run --project harness --python $(PYTHON_VERSION) python --version); \
	printf '%s\n' "$$python_version"; \
	case "$$python_version" in "Python $(PYTHON_VERSION)."*) ;; *) printf '%s\n' 'Python 3.12 is required.'; exit 1 ;; esac; \
	uv sync --project harness --python $(PYTHON_VERSION); \
	uv run --project harness --python $(PYTHON_VERSION) ruff --version; \
	uv run --project harness --python $(PYTHON_VERSION) pytest --version; \
	uv run --project harness --python $(PYTHON_VERSION) python -c 'import click, jsonschema, yaml'; \
	printf '%s\n' 'Python dependencies: click, jsonschema, pyyaml'; \
	uv run --project harness --python $(PYTHON_VERSION) gate --help >/dev/null; printf '%s\n' 'Gate CLI: ok'; \
	rustup --version; \
	active_toolchain=$$(rustup show active-toolchain); \
	printf '%s\n' "$$active_toolchain"; \
	case "$$active_toolchain" in stable*) ;; *) printf '%s\n' 'The active Rust toolchain must be stable.'; exit 1 ;; esac; \
	rustup run stable rustc --version; \
	rustup run stable cargo --version; \
	rustup run stable rustfmt --version; \
	rustup run stable cargo clippy --version; \
	cargo build --manifest-path processors/Cargo.toml --workspace; \
	grep -Fq 'scalaVersion := "2.12.20"' jobs/build.sbt; \
	grep -Fq 'sparkVersion = "3.5.5"' jobs/build.sbt; \
	grep -Fq 'local[2]' jobs/build.sbt; \
	sbt_opts="$${SBT_OPTS:-} $(SBT_REPOSITORY_FLAGS)"; \
	sbt_version=$$(cd jobs && SBT_OPTS="$$sbt_opts" sbt --version 2>&1); \
	printf '%s\n' "$$sbt_version"; \
	printf '%s\n' "$$sbt_version" | grep -Eq 'sbt version in this project:[[:space:]]*1\.'; \
	export SPARK_LOCAL_IP=127.0.0.1; \
	cd jobs && SBT_OPTS="$$sbt_opts" sbt -batch compile; \
	printf '%s\n' 'doctor: ok'

gate:
	@set -eu; \
	if [ -z "$(PROCESSOR)" ]; then printf '%s\n' 'usage: make gate PROCESSOR=<name> [BASE=<ref>] [BIN=<path>]'; exit 2; fi; \
	export PATH="$$HOME/.local/bin:$$HOME/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:$$PATH"; \
	set -- --processor "$(PROCESSOR)"; \
	if [ -n "$(BASE)" ]; then set -- "$$@" --base "$(BASE)"; fi; \
	if [ -n "$(BIN)" ]; then set -- "$$@" --bin "$(BIN)"; fi; \
	uv run --project harness --python $(PYTHON_VERSION) gate "$$@"

goldens:
	@set -eu; \
	export PATH="$$HOME/.local/bin:$$HOME/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:$$PATH"; \
	case "$$(uname -s)" in \
		Darwin) JAVA_HOME=$$(/usr/libexec/java_home -v 17);; \
		Linux) JAVA_HOME=$$(find /usr/lib/jvm -mindepth 1 -maxdepth 1 -type d -name '*17*' -print 2>/dev/null | sort | head -n 1);; \
		*) printf '%s\n' 'Unsupported operating system.'; exit 1;; \
	esac; \
	if [ -z "$$JAVA_HOME" ] || [ ! -x "$$JAVA_HOME/bin/java" ]; then printf '%s\n' 'JAVA_HOME must resolve to JDK 17.'; exit 1; fi; \
	export JAVA_HOME SPARK_LOCAL_IP=127.0.0.1; \
	case "$(JOB)" in \
		ota-status) project=otaStatus;; \
		charging-session) project=chargingSession;; \
		*) printf '%s\n' 'usage: make goldens JOB=ota-status|charging-session'; exit 1;; \
	esac; \
	rm -rf "goldens/$(JOB)" && mkdir -p "goldens/$(JOB)"; \
	sbt_opts="$${SBT_OPTS:-} $(SBT_REPOSITORY_FLAGS)"; \
	cd jobs && SBT_OPTS="$$sbt_opts" sbt -batch "$$project/run $(CURDIR)/fixtures/$(JOB) $(CURDIR)/goldens/$(JOB)"

fixtures:
	@set -eu; \
	export PATH="$$HOME/.local/bin:$$HOME/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:$$PATH"; \
	case "$$(uname -s)" in \
		Darwin) JAVA_HOME=$$(/usr/libexec/java_home -v 17);; \
		Linux) JAVA_HOME=$$(find /usr/lib/jvm -mindepth 1 -maxdepth 1 -type d -name '*17*' -print 2>/dev/null | sort | head -n 1);; \
		*) printf '%s\n' 'Unsupported operating system.'; exit 1;; \
	esac; \
	if [ -z "$$JAVA_HOME" ] || [ ! -x "$$JAVA_HOME/bin/java" ]; then printf '%s\n' 'JAVA_HOME must resolve to JDK 17.'; exit 1; fi; \
	export JAVA_HOME SPARK_LOCAL_IP=127.0.0.1; \
	uv run --project harness --python $(PYTHON_VERSION) python harness/src/harness/gen_fixtures.py --seed 20261026

test:
	@set -eu; \
	export PATH="$$HOME/.local/bin:$$HOME/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:$$PATH"; \
	case "$$(uname -s)" in \
		Darwin) JAVA_HOME=$$(/usr/libexec/java_home -v 17);; \
		Linux) JAVA_HOME=$$(find /usr/lib/jvm -mindepth 1 -maxdepth 1 -type d -name '*17*' -print 2>/dev/null | sort | head -n 1);; \
		*) printf '%s\n' 'Unsupported operating system.'; exit 1;; \
	esac; \
	if [ -z "$$JAVA_HOME" ] || [ ! -x "$$JAVA_HOME/bin/java" ]; then printf '%s\n' 'JAVA_HOME must resolve to JDK 17.'; exit 1; fi; \
	export JAVA_HOME SPARK_LOCAL_IP=127.0.0.1; \
	uv run --project harness --python $(PYTHON_VERSION) pytest harness/tests; \
	cargo test --manifest-path processors/Cargo.toml --workspace; \
	sbt_opts="$${SBT_OPTS:-} $(SBT_REPOSITORY_FLAGS)"; \
	cd jobs && SBT_OPTS="$$sbt_opts" sbt -batch test

fmt:
	@set -eu; \
	export PATH="$$HOME/.local/bin:$$HOME/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:$$PATH"; \
	case "$$(uname -s)" in \
		Darwin) JAVA_HOME=$$(/usr/libexec/java_home -v 17);; \
		Linux) JAVA_HOME=$$(find /usr/lib/jvm -mindepth 1 -maxdepth 1 -type d -name '*17*' -print 2>/dev/null | sort | head -n 1);; \
		*) printf '%s\n' 'Unsupported operating system.'; exit 1;; \
	esac; \
	if [ -z "$$JAVA_HOME" ] || [ ! -x "$$JAVA_HOME/bin/java" ]; then printf '%s\n' 'JAVA_HOME must resolve to JDK 17.'; exit 1; fi; \
	export JAVA_HOME SPARK_LOCAL_IP=127.0.0.1; \
	cargo fmt --manifest-path processors/Cargo.toml --all -- --check; \
	uv run --project harness --python $(PYTHON_VERSION) ruff format --check harness/src harness/tests

lint:
	@set -eu; \
	export PATH="$$HOME/.local/bin:$$HOME/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:$$PATH"; \
	case "$$(uname -s)" in \
		Darwin) JAVA_HOME=$$(/usr/libexec/java_home -v 17);; \
		Linux) JAVA_HOME=$$(find /usr/lib/jvm -mindepth 1 -maxdepth 1 -type d -name '*17*' -print 2>/dev/null | sort | head -n 1);; \
		*) printf '%s\n' 'Unsupported operating system.'; exit 1;; \
	esac; \
	if [ -z "$$JAVA_HOME" ] || [ ! -x "$$JAVA_HOME/bin/java" ]; then printf '%s\n' 'JAVA_HOME must resolve to JDK 17.'; exit 1; fi; \
	export JAVA_HOME SPARK_LOCAL_IP=127.0.0.1; \
	cargo clippy --manifest-path processors/Cargo.toml --workspace --all-targets -- -D warnings; \
	uv run --project harness --python $(PYTHON_VERSION) ruff check harness
