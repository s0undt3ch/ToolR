"""Fixtures for running GitHub Actions step scripts under `bash`."""

from __future__ import annotations

import shutil
import subprocess
from collections.abc import Callable
from collections.abc import Mapping
from pathlib import Path
from typing import Any

import pytest
import yaml

from tests._git import Git


def workflow_step(path: Path, job: str, step_id: str) -> dict[str, Any]:
    """Return step `step_id` in job `job` of the workflow at `path`."""
    data = yaml.safe_load(path.read_text())
    for step in data["jobs"][job]["steps"]:
        if step.get("id") == step_id:
            return step
    pytest.fail(f"no step with id `{step_id}` in job `{job}` of {path.name}")


def workflow_step_script(path: Path, job: str, step_id: str) -> str:
    """Return the `run:` body of step `step_id` in job `job` of the workflow at `path`."""
    return str(workflow_step(path, job, step_id)["run"])


def action_step_script(path: Path, step_id: str) -> str:
    """Return the `run:` body of step `step_id` in the composite action at `path`."""
    data = yaml.safe_load(path.read_text())
    for step in data["runs"]["steps"]:
        if step.get("id") == step_id:
            return str(step["run"])
    pytest.fail(f"no step with id `{step_id}` in {path}")


@pytest.fixture
def git_repo(tmp_path: Path, git: Git) -> Callable[..., Path]:
    """Return a factory that creates a git repository with one commit holding `files`."""

    def make(files: dict[str, str], name: str = "repo") -> Path:
        return git.make_repo(tmp_path / name, files=files)

    return make


StepRunner = Callable[[Path, Mapping[str, str]], subprocess.CompletedProcess[str]]


@pytest.fixture
def bash_step(tmp_path: Path) -> Callable[[str], StepRunner]:
    """Return a factory that turns a step's `run:` body into a runner.

    The runner takes the working directory and the step's environment, laid over
    `Git.env()`, and runs the body the way `shell: bash` does on a runner.
    """
    bash = shutil.which("bash")
    if bash is None:
        pytest.skip("bash not available")
    scripts = tmp_path / "steps"
    scripts.mkdir()

    def make(body: str) -> StepRunner:
        script = scripts / f"step-{len(list(scripts.iterdir()))}.sh"
        script.write_text(body)

        def run(cwd: Path, env: Mapping[str, str]) -> subprocess.CompletedProcess[str]:
            return subprocess.run(  # noqa: S603  # bash + script path are test-controlled
                [bash, "--noprofile", "--norc", "-eo", "pipefail", str(script)],
                cwd=cwd,
                env={**Git.env(), **env},
                capture_output=True,
                text=True,
                check=False,
                timeout=60,
            )

        return run

    return make
