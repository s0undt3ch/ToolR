"""Tests for the step of `release.yml` that pushes the release commit and tag.

The step's `run:` body is extracted from the workflow and run under `bash` in a
clone of a scratch bare repository standing in for `origin`.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

import pytest

from tests.workflows.conftest import git
from tests.workflows.conftest import git_env
from tests.workflows.conftest import workflow_step_script

REPO_ROOT = Path(__file__).resolve().parents[2]
RELEASE_YML = REPO_ROOT / ".github" / "workflows" / "release.yml"

pytestmark = pytest.mark.skipif(
    sys.platform == "win32", reason="the step runs under bash on ubuntu-latest only"
)


def configure_identity(repo: Path) -> None:
    git(repo, "config", "user.name", "Release Bot")
    git(repo, "config", "user.email", "release-bot@example.com")
    git(repo, "config", "commit.gpgsign", "false")
    git(repo, "config", "tag.gpgsign", "false")


@pytest.fixture
def release_remote(
    tmp_path: Path, git_repo: Callable[..., Path]
) -> Callable[..., tuple[Path, Path]]:
    """Return a factory for an `origin` bare repository and a release work dir.

    `origin`'s `main` holds commit A. The work dir is a clone at A with the release
    commit and the annotated `v9.9.9` tag on top, as `publish-release` leaves it
    before pushing. With `advance_main=True`, commit B is pushed to `origin`'s
    `main` from another clone, as if a PR merged during the release run.
    """

    def make(*, advance_main: bool) -> tuple[Path, Path]:
        seed = git_repo({"Cargo.toml": 'version = "9.9.8"\n'}, name="seed")
        origin = tmp_path / "origin.git"
        git(tmp_path, "clone", "-q", "--bare", str(seed), str(origin))

        workdir = tmp_path / "work"
        git(tmp_path, "clone", "-q", str(origin), str(workdir))
        configure_identity(workdir)
        (workdir / "Cargo.toml").write_text('version = "9.9.9"\n')
        git(workdir, "commit", "-q", "-am", "chore(release): Prepare for 9.9.9 release")
        git(workdir, "tag", "-m", "chore(release): Release v9.9.9", "-a", "v9.9.9")

        if advance_main:
            other = tmp_path / "other"
            git(tmp_path, "clone", "-q", str(origin), str(other))
            configure_identity(other)
            (other / "README.md").write_text("Merged during the release\n")
            git(other, "add", "README.md")
            git(other, "commit", "-q", "-m", "feat: merged during the release")
            git(other, "push", "-q", "origin", "HEAD:main")
        return origin, workdir

    return make


@pytest.fixture
def run_push_step(tmp_path: Path) -> Callable[..., subprocess.CompletedProcess[str]]:
    """Return a callable that runs the push step in `workdir`."""
    bash = shutil.which("bash")
    if bash is None:
        pytest.skip("bash not available")
    script = tmp_path / "push-step.sh"
    script.write_text(workflow_step_script(RELEASE_YML, "publish-release", "push-release"))

    def run(
        workdir: Path,
        *,
        ref: str = "refs/heads/main",
        ref_name: str = "main",
    ) -> subprocess.CompletedProcess[str]:
        env = git_env()
        env["RELEASE_VERSION"] = "9.9.9"
        env["GITHUB_REF"] = ref
        env["GITHUB_REF_NAME"] = ref_name
        # Mirrors the `shell: bash` invocation GitHub Actions uses.
        return subprocess.run(  # noqa: S603  # bash + script path are test-controlled
            [bash, "--noprofile", "--norc", "-eo", "pipefail", str(script)],
            cwd=workdir,
            env=env,
            capture_output=True,
            text=True,
            check=False,
            timeout=60,
        )

    return run


def remote_sha(origin: Path, ref: str) -> str:
    return git(origin, "rev-parse", ref).strip()


def test_push_publishes_nothing_when_branch_moved(
    release_remote: Callable[..., tuple[Path, Path]],
    run_push_step: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    origin, workdir = release_remote(advance_main=True)
    moved_main = remote_sha(origin, "refs/heads/main")

    result = run_push_step(workdir)

    assert result.returncode != 0, result.stdout + result.stderr
    assert git(origin, "tag", "--list") == ""
    assert remote_sha(origin, "refs/heads/main") == moved_main
    assert "dispatch the release again" in result.stderr


def test_push_publishes_branch_and_tag_together(
    release_remote: Callable[..., tuple[Path, Path]],
    run_push_step: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    origin, workdir = release_remote(advance_main=False)
    release_commit = git(workdir, "rev-parse", "HEAD").strip()

    result = run_push_step(workdir)

    assert result.returncode == 0, result.stdout + result.stderr
    assert remote_sha(origin, "refs/heads/main") == release_commit
    assert remote_sha(origin, "refs/tags/v9.9.9^{commit}") == release_commit


def test_push_refuses_non_branch_ref(
    release_remote: Callable[..., tuple[Path, Path]],
    run_push_step: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    origin, workdir = release_remote(advance_main=False)
    refs_before = git(origin, "show-ref")

    result = run_push_step(workdir, ref="refs/tags/v9.9.8", ref_name="v9.9.8")

    assert result.returncode != 0, result.stdout + result.stderr
    assert "refs/heads/" in result.stderr
    assert git(origin, "show-ref") == refs_before
