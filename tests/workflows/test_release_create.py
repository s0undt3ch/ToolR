"""Tests for the step of `release.yml` that creates the GitHub release.

The step's `run:` body is extracted from the workflow and run under `bash` with a
stub `gh` on `PATH` that records its arguments. A pre-release must be created
with `--prerelease` and without `--latest`: GitHub's latest-release endpoint,
which the action and the install scripts resolve `latest` through, then skips it.
"""

from __future__ import annotations

import os
import sys
from collections.abc import Callable
from pathlib import Path

import pytest

from tests.workflows.conftest import StepRunner
from tests.workflows.conftest import workflow_step_script

REPO_ROOT = Path(__file__).resolve().parents[2]
RELEASE_YML = REPO_ROOT / ".github" / "workflows" / "release.yml"

pytestmark = pytest.mark.skipif(
    sys.platform == "win32", reason="the step runs under bash on ubuntu-latest only"
)


@pytest.fixture
def run_create_step(
    tmp_path: Path, bash_step: Callable[[str], StepRunner]
) -> Callable[[str, str], list[str]]:
    """Return a callable that runs the step and returns the arguments `gh` got."""
    step = bash_step(workflow_step_script(RELEASE_YML, "publish-release", "create-release"))
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    args_file = tmp_path / "gh-args"
    gh = bin_dir / "gh"
    gh.write_text(f'#!/bin/sh\nprintf "%s\\n" "$@" > "{args_file}"\n')
    gh.chmod(0o755)
    workdir = tmp_path / "work"
    for name in ("dist/toolr-1.whl", "release-assets/toolr-1.tar.gz", "notes.md"):
        (workdir / name).parent.mkdir(parents=True, exist_ok=True)
        (workdir / name).write_text("")

    def run(version: str, prerelease: str) -> list[str]:
        result = step(
            workdir,
            {
                "PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}",
                "GH_TOKEN": "token",
                "GITHUB_REPOSITORY": "s0undt3ch/ToolR",
                "RELEASE_VERSION": version,
                "RELEASE_NOTES": "notes.md",
                "RELEASE_PRERELEASE": prerelease,
            },
        )
        assert result.returncode == 0, result.stderr
        return args_file.read_text().splitlines()

    return run


def test_prerelease_is_not_marked_latest(run_create_step: Callable[[str, str], list[str]]) -> None:
    args = run_create_step("1.0.0-rc.1", "true")
    assert args[:2] == ["release", "create"]
    assert "v1.0.0-rc.1" in args
    assert "--prerelease" in args
    assert "--latest" not in args


def test_final_release_is_marked_latest(run_create_step: Callable[[str, str], list[str]]) -> None:
    args = run_create_step("1.0.0", "false")
    assert "v1.0.0" in args
    assert "--latest" in args
    assert "--prerelease" not in args
    assert args[-2:] == ["dist/toolr-1.whl", "release-assets/toolr-1.tar.gz"]
