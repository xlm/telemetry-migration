"""Choose which Processors a branch must run the Gate for."""

from __future__ import annotations

import json
import subprocess
from collections.abc import Iterable
from pathlib import Path, PurePosixPath

import click

from harness.common import PROCESSORS, ROOT
from harness.gate import path_is_protected

GATE_DEFINITION_PATHS = ("Makefile",)


def processors_to_gate(paths: Iterable[str], processors: Iterable[str] = PROCESSORS) -> list[str]:
    """Return the Processors whose Gate result the changed paths can affect.

    A Gate input, the Gate's own definition, or Rust shared across Processors
    affects every Processor. A Processor's crate or Processor Manifest affects
    only that Processor. Anything else affects none.
    """
    names = sorted(processors)
    selected = set()
    for path in paths:
        normalized = PurePosixPath(path).as_posix()
        if path_is_protected(normalized) or normalized in GATE_DEFINITION_PATHS:
            return names
        owner = _owning_processor(normalized, names)
        if owner:
            selected.add(owner)
        elif normalized.startswith("processors/"):
            return names
    return [name for name in names if name in selected]


def _owning_processor(path: str, names: list[str]) -> str | None:
    for name in names:
        if path.startswith(f"processors/{name}/") or path == f"manifests/{name}.processor.yaml":
            return name
    return None


def changed_paths(base: str, *, root: Path = ROOT) -> list[str]:
    result = subprocess.run(
        ["git", "diff", "--name-only", "--no-renames", "-z", f"{base}...HEAD"],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        message = result.stderr.strip() or f"git exited {result.returncode}"
        raise click.ClickException(f"git diff against {base} failed: {message}")
    return [path for path in result.stdout.split("\0") if path]


@click.command()
@click.option(
    "--base", default="origin/main", show_default=True, help="Git base the branch is compared with."
)
def main(base: str) -> None:
    """Print a JSON list of the Processors this branch must run the Gate for."""
    click.echo(json.dumps(processors_to_gate(changed_paths(base))))
