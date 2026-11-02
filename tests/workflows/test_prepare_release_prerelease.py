"""Tests that a pre-release leaves its changes for the final release to roll up.

An rc gets a GitHub release body, but neither a CHANGELOG section nor a cleared
release-notes queue: `_prepare-release.yml` gates those two steps on
`release-prerelease`, and `cliff.toml`'s `tag_pattern` only treats final
release tags as changelog sections.
"""

from __future__ import annotations

import shutil
import subprocess
import tomllib
from pathlib import Path
from typing import Any

import pytest
import yaml

from tests.workflows.conftest import configure_identity
from tests.workflows.conftest import git
from tests.workflows.conftest import git_env

REPO_ROOT = Path(__file__).resolve().parents[2]
PREPARE_RELEASE_YML = REPO_ROOT / ".github" / "workflows" / "_prepare-release.yml"
CLIFF_TOML = REPO_ROOT / "cliff.toml"
FINAL_ONLY = "steps.bump-version.outputs.release-prerelease != 'true'"


def _steps() -> list[dict[str, Any]]:
    data = yaml.safe_load(PREPARE_RELEASE_YML.read_text())
    return data["jobs"]["prepare-release"]["steps"]


def _step(step_id: str) -> dict[str, Any]:
    for step in _steps():
        if step.get("id") == step_id:
            return step
    pytest.fail(f"no step with id `{step_id}` in {PREPARE_RELEASE_YML.name}")


@pytest.mark.parametrize("step_id", ["update-changelog", "clear-release-notes"])
def test_final_release_only_steps(step_id: str) -> None:
    assert _step(step_id).get("if") == FINAL_ONLY


@pytest.mark.parametrize("step_id", ["git-cliff", "prepare-release-notes", "commit-release"])
def test_prerelease_still_renders_and_commits(step_id: str) -> None:
    """The rc's GitHub release body is still rendered, and the release commit made."""
    assert "if" not in _step(step_id)


def test_release_prerelease_is_exported() -> None:
    data = yaml.safe_load(PREPARE_RELEASE_YML.read_text())
    job_outputs = data["jobs"]["prepare-release"]["outputs"]
    assert (
        job_outputs["release-prerelease"] == "${{ steps.bump-version.outputs.release-prerelease }}"
    )
    workflow_outputs = data[True]["workflow_call"]["outputs"]  # PyYAML reads `on:` as True
    assert "release-prerelease" in workflow_outputs


def test_cliff_tag_pattern_matches_final_releases_only() -> None:
    pattern = tomllib.loads(CLIFF_TOML.read_text())["git"]["tag_pattern"]
    assert pattern == r"^v[0-9]+\.[0-9]+\.[0-9]+$"


@pytest.fixture
def rc_history(tmp_path: Path) -> Path:
    """A repository with `v0.35.1`, then `v1.0.0-rc.1`, then one more fix."""
    repo = tmp_path / "repo"
    repo.mkdir()
    git(repo, "init", "-q", "-b", "main")
    configure_identity(repo)

    def commit(message: str) -> None:
        git(repo, "commit", "-q", "--allow-empty", "-m", message)

    commit("feat: base feature")
    git(repo, "tag", "-a", "-m", "v0.35.1", "v0.35.1")
    commit("feat: before the rc")
    git(repo, "tag", "-a", "-m", "v1.0.0-rc.1", "v1.0.0-rc.1")
    commit("fix: after the rc")
    return repo


def _git_cliff(repo: Path, *args: str) -> str:
    git_cliff = shutil.which("git-cliff")
    if git_cliff is None:
        pytest.skip("git-cliff not available")
    return subprocess.run(  # noqa: S603  # git-cliff binary + args are test-controlled
        [git_cliff, "--config", str(CLIFF_TOML), *args],
        cwd=repo,
        env=git_env(),
        capture_output=True,
        text=True,
        check=True,
    ).stdout


def test_final_release_rolls_up_prerelease(rc_history: Path) -> None:
    """The final release's section, as `Update CHANGELOG.md` renders it, covers the rc's commits."""
    section = _git_cliff(rc_history, "--unreleased", "--tag", "1.0.0")
    assert "## 1.0.0" in section
    assert "Before the rc" in section
    assert "After the rc" in section
    assert "Base feature" not in section
    assert "rc.1" not in section


def test_tagged_history_has_no_prerelease_section(rc_history: Path) -> None:
    git(rc_history, "tag", "-a", "-m", "v1.0.0", "v1.0.0")
    changelog = _git_cliff(rc_history)
    assert "## 1.0.0 " in changelog
    assert "## 0.35.1 " in changelog
    assert "1.0.0-rc.1" not in changelog
