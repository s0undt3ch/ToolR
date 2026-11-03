"""Helpers for tests that build real git repositories in `tmp_path`."""

from __future__ import annotations

import os
import shutil
import subprocess
from collections.abc import Mapping
from collections.abc import Sequence
from pathlib import Path

import pytest


def require_git() -> None:
    """Skip the calling test when `git` isn't installed."""
    if shutil.which("git") is None:
        pytest.skip("git not available")


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
        encoding="utf-8",
        check=True,
    ).stdout


def configure_identity(repo: Path) -> None:
    """Give `repo` a committer identity and turn off commit and tag signing."""
    git(repo, "config", "user.name", "Release Bot")
    git(repo, "config", "user.email", "release-bot@example.com")
    git(repo, "config", "commit.gpgsign", "false")
    git(repo, "config", "tag.gpgsign", "false")


def make_repo(
    path: Path,
    commits: Sequence[str] = ("initial",),
    tags: Mapping[str, int] | None = None,
    files: Mapping[str, str] | None = None,
) -> Path:
    """Create a git repository at `path` and return it.

    `files` are written and committed by the first commit; the other commits are
    empty. `tags` maps an annotated tag name to the index of the commit it marks.
    """
    path.mkdir()
    git(path, "init", "-q", "-b", "main")
    configure_identity(path)
    for relative, content in (files or {}).items():
        target = path / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content)
    if files:
        git(path, "add", "--", *files)
    tags_by_commit: dict[int, list[str]] = {}
    for tag, index in (tags or {}).items():
        tags_by_commit.setdefault(index, []).append(tag)
    for index, message in enumerate(commits):
        git(path, "commit", "-q", "--allow-empty", "-m", message)
        for tag in tags_by_commit.get(index, []):
            git(path, "tag", "-a", "-m", f"Release {tag}", tag)
    return path
