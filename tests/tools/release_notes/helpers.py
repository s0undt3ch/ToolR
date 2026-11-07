"""Shared helpers for the release-notes command tests."""

from __future__ import annotations

from pathlib import Path

from tests._git import Git

VALID_NOTE_NAME = "2026-11-01.14-03-27.foo.md"
FEATURE_NOTE_PATH = "release-notes/2026-11-01.15-00-00.bar.md"
LONG_NOTE = (
    "Fixed `Optional[bool]` handling; see [link](url) and ~/src/[wip]/ for details. "
    + "word " * 30
    + "end :smile:"
)


def staged(git: Git, repo: Path) -> list[str]:
    return git(repo, "diff", "--cached", "--name-only").split()


def notes(repo: Path) -> list[Path]:
    return sorted((repo / "release-notes").glob("*.md"))
