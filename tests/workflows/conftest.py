"""Fixtures for running GitHub Actions step scripts under `bash`."""

from __future__ import annotations

import os
import shutil
import subprocess
from collections.abc import Callable
from collections.abc import Mapping
from pathlib import Path

import pytest
import yaml


def git_env() -> dict[str, str]:
    """Environment for `git` that ignores the developer's global and system config."""
    env = dict(os.environ)
    env["GIT_CONFIG_GLOBAL"] = os.devnull
    env["GIT_CONFIG_NOSYSTEM"] = "1"
    return env


def git(repo: Path, *args: str) -> str:
    """Run `git` in `repo` and return its stdout."""
    git_bin = shutil.which("git")
    assert git_bin is not None, "git is not available"
    return subprocess.run(  # noqa: S603  # git binary + args are test-controlled
        [git_bin, *args],
        cwd=repo,
        env=git_env(),
        capture_output=True,
        text=True,
        check=True,
    ).stdout


def configure_identity(repo: Path) -> None:
    """Give `repo` a committer identity and turn off commit and tag signing."""
    git(repo, "config", "user.name", "Release Bot")
    git(repo, "config", "user.email", "release-bot@example.com")
    git(repo, "config", "commit.gpgsign", "false")
    git(repo, "config", "tag.gpgsign", "false")


def workflow_step_script(path: Path, job: str, step_id: str) -> str:
    """Return the `run:` body of step `step_id` in job `job` of the workflow at `path`."""
    data = yaml.safe_load(path.read_text())
    for step in data["jobs"][job]["steps"]:
        if step.get("id") == step_id:
            return str(step["run"])
    pytest.fail(f"no step with id `{step_id}` in job `{job}` of {path.name}")


def action_step_script(path: Path, step_id: str) -> str:
    """Return the `run:` body of step `step_id` in the composite action at `path`."""
    data = yaml.safe_load(path.read_text())
    for step in data["runs"]["steps"]:
        if step.get("id") == step_id:
            return str(step["run"])
    pytest.fail(f"no step with id `{step_id}` in {path}")


@pytest.fixture
def git_repo(tmp_path: Path) -> Callable[..., Path]:
    """Return a factory that creates a git repository with one commit holding `files`."""
    if shutil.which("git") is None:
        pytest.skip("git not available")

    def make(files: dict[str, str], name: str = "repo") -> Path:
        repo = tmp_path / name
        repo.mkdir()
        git(repo, "init", "-q", "-b", "main")
        configure_identity(repo)
        for relative, content in files.items():
            target = repo / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(content)
        git(repo, "add", "--", *files)
        git(repo, "commit", "-q", "-m", "initial")
        return repo

    return make


StepRunner = Callable[[Path, Mapping[str, str]], subprocess.CompletedProcess[str]]


@pytest.fixture
def bash_step(tmp_path: Path) -> Callable[[str], StepRunner]:
    """Return a factory that turns a step's `run:` body into a runner.

    The runner takes the working directory and the step's environment, laid over
    `git_env()`, and runs the body the way `shell: bash` does on a runner.
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
                env={**git_env(), **env},
                capture_output=True,
                text=True,
                check=False,
                timeout=60,
            )

        return run

    return make
