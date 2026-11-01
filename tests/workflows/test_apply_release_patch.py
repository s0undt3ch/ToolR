"""Tests for the `apply-release-patch` composite action's apply step.

The step's `run:` body is extracted from `action.yml` and run under `bash`
against a scratch repository holding a release patch.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

import pytest

from tests.workflows.conftest import action_step_script
from tests.workflows.conftest import git
from tests.workflows.conftest import git_env

REPO_ROOT = Path(__file__).resolve().parents[2]
APPLY_RELEASE_PATCH_YML = REPO_ROOT / ".github" / "actions" / "apply-release-patch" / "action.yml"
PATCH_NAME = "toolr-9.9.9.patch"

pytestmark = pytest.mark.skipif(
    sys.platform == "win32", reason="the step runs under bash on ubuntu-latest only"
)


@dataclass(frozen=True)
class PatchRepo:
    path: Path
    dispatched: str
    later: str


@pytest.fixture
def patch_repo(git_repo: Callable[..., Path]) -> PatchRepo:
    """A repository with commit A, its child B, and a release patch made on A.

    The patch sits untracked in the work tree as `toolr-9.9.9.patch`, where the
    download step leaves it. `HEAD` is left at B.
    """
    repo = git_repo({"a.txt": "a\n", "Cargo.toml": 'version = "9.9.8"\n'})
    dispatched = git(repo, "rev-parse", "HEAD").strip()

    (repo / "Cargo.toml").write_text('version = "9.9.9"\n')
    git(repo, "commit", "-q", "-am", "chore(release): Prepare for 9.9.9 release")
    patch = git(repo, "format-patch", "--keep-subject", "--binary", "--stdout", "HEAD^")
    git(repo, "reset", "-q", "--hard", dispatched)

    (repo / "b.txt").write_text("b\n")
    git(repo, "add", "b.txt")
    git(repo, "commit", "-q", "-m", "feat: merged after dispatch")
    later = git(repo, "rev-parse", "HEAD").strip()

    (repo / PATCH_NAME).write_text(patch)
    return PatchRepo(path=repo, dispatched=dispatched, later=later)


@pytest.fixture
def run_apply_step(tmp_path: Path) -> Callable[[Path, str], subprocess.CompletedProcess[str]]:
    """Return a callable that runs the apply step in `repo` with `GITHUB_SHA` set."""
    bash = shutil.which("bash")
    if bash is None:
        pytest.skip("bash not available")
    script = tmp_path / "apply-step.sh"
    script.write_text(action_step_script(APPLY_RELEASE_PATCH_YML, "apply"))

    def run(repo: Path, github_sha: str) -> subprocess.CompletedProcess[str]:
        env = git_env()
        env["RELEASE_PATCH_NAME"] = PATCH_NAME
        env["GITHUB_SHA"] = github_sha
        # Mirrors the `shell: bash` invocation GitHub Actions uses.
        return subprocess.run(  # noqa: S603  # bash + script path are test-controlled
            [bash, "--noprofile", "--norc", "-eo", "pipefail", str(script)],
            cwd=repo,
            env=env,
            capture_output=True,
            text=True,
            check=False,
            timeout=60,
        )

    return run


def test_apply_on_dispatched_commit(
    patch_repo: PatchRepo,
    run_apply_step: Callable[[Path, str], subprocess.CompletedProcess[str]],
) -> None:
    git(patch_repo.path, "checkout", "-q", patch_repo.dispatched)

    result = run_apply_step(patch_repo.path, patch_repo.dispatched)

    assert result.returncode == 0, result.stdout + result.stderr
    assert git(patch_repo.path, "rev-parse", "HEAD^").strip() == patch_repo.dispatched
    assert (patch_repo.path / "Cargo.toml").read_text() == 'version = "9.9.9"\n'


def test_apply_refuses_other_base(
    patch_repo: PatchRepo,
    run_apply_step: Callable[[Path, str], subprocess.CompletedProcess[str]],
) -> None:
    result = run_apply_step(patch_repo.path, patch_repo.dispatched)

    assert result.returncode != 0, result.stdout + result.stderr
    assert "::error::" in result.stderr
    assert patch_repo.dispatched in result.stderr
    assert patch_repo.later in result.stderr
