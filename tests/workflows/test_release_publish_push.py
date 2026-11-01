"""Tests for the step of `release.yml` that pushes the release commit and tag.

The step's `run:` body is extracted from the workflow and run under `bash` in a
clone of a scratch bare repository standing in for `origin`.
"""

from __future__ import annotations

import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

import pytest

from tests.workflows.conftest import StepRunner
from tests.workflows.conftest import configure_identity
from tests.workflows.conftest import git
from tests.workflows.conftest import workflow_step_script

REPO_ROOT = Path(__file__).resolve().parents[2]
RELEASE_YML = REPO_ROOT / ".github" / "workflows" / "release.yml"

pytestmark = pytest.mark.skipif(
    sys.platform == "win32", reason="the step runs under bash on ubuntu-latest only"
)


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
def run_push_step(
    bash_step: Callable[[str], StepRunner],
) -> Callable[..., subprocess.CompletedProcess[str]]:
    """Return a callable that runs the push step in `workdir`."""
    step = bash_step(workflow_step_script(RELEASE_YML, "publish-release", "push-release"))

    def run(
        workdir: Path,
        *,
        ref: str = "refs/heads/main",
        ref_name: str = "main",
    ) -> subprocess.CompletedProcess[str]:
        env = {
            "RELEASE_VERSION": "9.9.9",
            "GITHUB_REF": ref,
            "GITHUB_REF_NAME": ref_name,
            # The release commit sits on the commit the run was dispatched on.
            "GITHUB_SHA": git(workdir, "rev-parse", "HEAD^").strip(),
        }
        return step(workdir, env)

    return run


def remote_sha(origin: Path, ref: str) -> str:
    return git(origin, "rev-parse", ref).strip()


def test_push_non_branch_ref_is_refused(
    release_remote: Callable[..., tuple[Path, Path]],
    run_push_step: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    origin, workdir = release_remote(advance_main=False)
    refs_before = git(origin, "show-ref")

    result = run_push_step(workdir, ref="refs/tags/v9.9.8", ref_name="v9.9.8")

    assert result.returncode != 0, result.stdout + result.stderr
    assert "refs/heads/" in result.stderr
    assert git(origin, "show-ref") == refs_before


def test_push_branch_current_publishes_branch_and_tag(
    release_remote: Callable[..., tuple[Path, Path]],
    run_push_step: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    origin, workdir = release_remote(advance_main=False)
    release_commit = git(workdir, "rev-parse", "HEAD").strip()

    result = run_push_step(workdir)

    assert result.returncode == 0, result.stdout + result.stderr
    assert remote_sha(origin, "refs/heads/main") == release_commit
    assert remote_sha(origin, "refs/tags/v9.9.9^{commit}") == release_commit


def test_push_origin_unreachable_reports_unknown_state(
    release_remote: Callable[..., tuple[Path, Path]],
    run_push_step: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    origin, workdir = release_remote(advance_main=False)
    origin.rename(origin.with_name("gone.git"))

    result = run_push_step(workdir)

    assert result.returncode != 0, result.stdout + result.stderr
    assert "Pushing v9.9.9 failed: origin could not be queried" in result.stderr
    assert "nothing was published" not in result.stderr
    assert "earlier attempt" not in result.stderr


def test_push_tag_on_release_commit_reports_earlier_attempt(
    release_remote: Callable[..., tuple[Path, Path]],
    run_push_step: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """A re-run after a later step failed finds the tag it pushed the first time."""
    origin, workdir = release_remote(advance_main=False)
    first = run_push_step(workdir)
    assert first.returncode == 0, first.stdout + first.stderr
    published_tag = remote_sha(origin, "refs/tags/v9.9.9")
    # The re-run tags again, so the local tag object differs from the published one.
    git(workdir, "tag", "-f", "-m", "chore(release): Release v9.9.9 (re-run)", "-a", "v9.9.9")

    result = run_push_step(workdir)

    assert result.returncode != 0, result.stdout + result.stderr
    assert (
        "Pushing v9.9.9 failed: origin already has v9.9.9 on this release commit" in result.stderr
    )
    assert '"Create GitHub release"' in result.stderr
    assert "nothing was published" not in result.stderr
    assert remote_sha(origin, "refs/tags/v9.9.9") == published_tag


def test_push_tag_on_other_commit_stops(
    tmp_path: Path,
    release_remote: Callable[..., tuple[Path, Path]],
    run_push_step: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    origin, workdir = release_remote(advance_main=False)
    other = tmp_path / "other"
    git(tmp_path, "clone", "-q", str(origin), str(other))
    configure_identity(other)
    git(other, "tag", "-m", "unrelated", "-a", "v9.9.9")
    git(other, "push", "-q", "origin", "refs/tags/v9.9.9")
    refs_before = git(origin, "show-ref")

    result = run_push_step(workdir)

    assert result.returncode != 0, result.stdout + result.stderr
    assert "Pushing v9.9.9 failed: origin has v9.9.9 on another commit" in result.stderr
    # Someone else's tag: deleting it to retry could destroy a real release.
    assert "delete" not in result.stderr
    assert "earlier attempt" not in result.stderr
    assert git(origin, "show-ref") == refs_before


def test_push_branch_moved_publishes_nothing(
    release_remote: Callable[..., tuple[Path, Path]],
    run_push_step: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    origin, workdir = release_remote(advance_main=True)
    moved_main = remote_sha(origin, "refs/heads/main")

    result = run_push_step(workdir)

    assert result.returncode != 0, result.stdout + result.stderr
    assert git(origin, "tag", "--list") == ""
    assert remote_sha(origin, "refs/heads/main") == moved_main
    assert (
        "Pushing v9.9.9 failed: origin's main moved since the run was dispatched" in result.stderr
    )
    assert "Dispatch the release again" in result.stderr


def test_push_branch_unchanged_reports_rerun_safe(
    release_remote: Callable[..., tuple[Path, Path]],
    run_push_step: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    origin, workdir = release_remote(advance_main=False)
    hook = origin / "hooks" / "pre-receive"
    hook.write_text("#!/bin/sh\necho 'rejected by hook' >&2\nexit 1\n")
    hook.chmod(0o755)
    refs_before = git(origin, "show-ref")

    result = run_push_step(workdir)

    assert result.returncode != 0, result.stdout + result.stderr
    assert "Pushing v9.9.9 failed: origin's main has not moved" in result.stderr
    assert "Re-run this job" in result.stderr
    assert "moved since the run was dispatched" not in result.stderr
    assert git(origin, "show-ref") == refs_before
