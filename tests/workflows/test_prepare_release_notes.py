"""Tests for the `Prepare GH Release changelog` step of `_prepare-release.yml`.

The step's `run:` body runs under `bash` with a stub `toolr` on `PATH` that records its arguments.
"""

from __future__ import annotations

import os
from collections.abc import Callable
from pathlib import Path

import pytest

from tests.workflows.conftest import StepRunner
from tests.workflows.conftest import workflow_step_script

REPO_ROOT = Path(__file__).resolve().parents[2]
PREPARE_RELEASE_YML = REPO_ROOT / ".github" / "workflows" / "_prepare-release.yml"

pytestmark = pytest.mark.skip_on_windows(reason="the step runs under bash on ubuntu-latest only")


@pytest.fixture
def run_step(tmp_path: Path, bash_step: Callable[[str], StepRunner]) -> Callable[[str], list[str]]:
    """Return a runner: run the step with `RELEASE_PRERELEASE` set, return the stub's arguments."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    calls = tmp_path / "toolr-args"
    toolr = bin_dir / "toolr"
    toolr.write_text(f'#!/usr/bin/env bash\nprintf "%s\\n" "$@" > "{calls}"\n')
    toolr.chmod(0o755)
    step = bash_step(
        workflow_step_script(PREPARE_RELEASE_YML, "prepare-release", "prepare-release-notes")
    )

    def run(prerelease: str) -> list[str]:
        output = tmp_path / "github-output"
        result = step(
            tmp_path,
            {
                "PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}",
                "RELEASE_VERSION": "1.2.0",
                "GITHUB_REPO": "o/r",
                "RELEASE_PRERELEASE": prerelease,
                "GITHUB_OUTPUT": str(output),
            },
        )
        assert result.returncode == 0, result.stderr
        assert output.read_text() == "release-notes-filename=release-notes.md\n"
        return calls.read_text().splitlines()

    return run


def test_final_release(run_step: Callable[[str], list[str]]) -> None:
    assert run_step("false") == [
        "release-notes",
        "github-body",
        "1.2.0",
        "o/r",
        "release-notes.md",
        "--rendered",
        ".release-changes.md",
    ]


def test_prerelease(run_step: Callable[[str], list[str]]) -> None:
    assert run_step("true")[-1] == "--prerelease"
