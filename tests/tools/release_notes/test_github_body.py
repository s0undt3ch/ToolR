"""Tests for ``toolr release-notes github-body``, against the real ``cliff.toml`` and ``git-cliff``."""

from __future__ import annotations

import os
import shutil
from collections.abc import Callable
from pathlib import Path

import pytest

from toolr.testing import make_context

from tools.release_notes import GITHUB_BODY_MAX_CHARS
from tools.release_notes import github_body

from tests._git import Git

REPO_ROOT = Path(__file__).resolve().parents[3]
CHANGELOG_URL = "https://github.com/o/r/blob/v1.2.0/CHANGELOG.md"


@pytest.fixture(autouse=True)
def _git_cliff_offline(monkeypatch: pytest.MonkeyPatch) -> None:
    if shutil.which("git-cliff") is None:
        # mise.toml pins git-cliff, so CI always has it; only a local run may not.
        if os.environ.get("CI"):
            pytest.fail("git-cliff is not on PATH; it is pinned in mise.toml")
        pytest.skip("git-cliff not available")
    monkeypatch.setenv("GIT_CLIFF_OFFLINE", "true")


@pytest.fixture
def release_repo(tmp_path: Path, git: Git) -> Callable[[str], Path]:
    """Factory: a repository with ``cliff.toml``, one queued note and a few commits."""

    def make(note: str) -> Path:
        return git.make_repo(
            tmp_path / "repo",
            commits=["feat: add a widget", "docs: explain the widget", "fix: mend the widget"],
            files={
                "cliff.toml": (REPO_ROOT / "cliff.toml").read_text(encoding="utf-8"),
                "release-notes/2026-11-01.14-03-27.widget.md": note,
            },
        )

    return make


def test_body_is_notes_and_link(release_repo: Callable[[str], Path], tmp_path: Path) -> None:
    repo = release_repo("A widget now exists.\n")
    output = tmp_path / "body.md"
    rendered = tmp_path / "rendered.md"
    github_body(make_context(repo), "1.2.0", "o/r", str(output), rendered=str(rendered))

    body = output.read_text(encoding="utf-8")
    assert body.startswith("### Notes\n\nA widget now exists.\n")
    assert f"[the full list of changes]({CHANGELOG_URL})" in body
    assert "Add a widget" not in body
    assert "## Unreleased" not in body
    # `clear --rendered` reads this file and needs the heading.
    assert "### Notes" in rendered.read_text(encoding="utf-8").splitlines()


def test_prerelease_links_commit_history(
    release_repo: Callable[[str], Path], tmp_path: Path
) -> None:
    repo = release_repo("A widget now exists.\n")
    output = tmp_path / "body.md"
    github_body(
        make_context(repo),
        "1.2.0-rc.1",
        "o/r",
        str(output),
        rendered=str(tmp_path / "rendered.md"),
        prerelease=True,
    )
    assert "(https://github.com/o/r/commits/v1.2.0-rc.1)" in output.read_text(encoding="utf-8")


def test_too_long_body_is_only_the_link(
    release_repo: Callable[[str], Path], tmp_path: Path
) -> None:
    repo = release_repo("word " * (GITHUB_BODY_MAX_CHARS // 5) + "\n")
    output = tmp_path / "body.md"
    github_body(
        make_context(repo), "1.2.0", "o/r", str(output), rendered=str(tmp_path / "rendered.md")
    )
    assert output.read_text(encoding="utf-8") == (
        f"The release notes are too long for a GitHub release. See [the full list of changes]({CHANGELOG_URL}).\n"
    )


def test_git_cliff_failure_exits(git: Git, tmp_path: Path) -> None:
    repo = git.make_repo(
        tmp_path / "repo", commits=["feat: add a widget"], files={"cliff.toml": "[changelog\n"}
    )
    output = tmp_path / "body.md"
    with pytest.raises(SystemExit) as exc:
        github_body(
            make_context(repo), "1.2.0", "o/r", str(output), rendered=str(tmp_path / "r.md")
        )
    assert exc.value.code != 0
    assert not output.exists()
