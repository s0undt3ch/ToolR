"""Shared helpers for the release-notes command tests."""

from __future__ import annotations

import subprocess
from pathlib import Path

VALID_NOTE_NAME = "2026-11-01.14-03-27.foo.md"
FEATURE_NOTE_PATH = "release-notes/2026-11-01.15-00-00.bar.md"
LONG_NOTE = (
    "Fixed `Optional[bool]` handling; see [link](url) and ~/src/[wip]/ for details. "
    + "word " * 30
    + "end :smile:"
)


def git(repo: Path, *args: str) -> str:
    cmd = [
        "git",
        "-C",
        str(repo),
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.hooksPath=/dev/null",
        *args,
    ]
    return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout  # noqa: S603


def staged(repo: Path) -> list[str]:
    return git(repo, "diff", "--cached", "--name-only").split()


def notes(repo: Path) -> list[Path]:
    return sorted((repo / "release-notes").glob("*.md"))


def commit(repo: Path, message: str, files: dict[str, str | None] | None = None) -> None:
    for name, text in (files or {}).items():
        path = repo / name
        if text is None:
            git(repo, "rm", "-q", "--", name)
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        git(repo, "add", "--", name)
    git(repo, "commit", "--allow-empty", "-q", "-m", message)
