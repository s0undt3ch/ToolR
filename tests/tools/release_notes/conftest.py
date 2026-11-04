"""Fixtures for the release-notes command tests."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

import pytest

from tests._git import configure_identity
from tests._git import git
from tests.tools.release_notes.helpers import VALID_NOTE_NAME
from tests.tools.release_notes.helpers import commit


@pytest.fixture
def notes_dir(tmp_path: Path) -> Callable[[dict[str, str]], Path]:
    """Factory: write ``files`` into ``tmp_path/release-notes``. Returns the directory."""

    def _make(files: dict[str, str]) -> Path:
        directory = tmp_path / "release-notes"
        directory.mkdir()
        for name, text in files.items():
            path = directory / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(text.encode("utf-8"))
        return directory

    return _make


@pytest.fixture
def git_repo(tmp_path: Path) -> Callable[..., Path]:
    """Factory: a repo with one commit on ``main``, checked out to ``branch``."""

    def _make(branch: str = "main") -> Path:
        git(tmp_path, "init", "-b", "main")
        configure_identity(tmp_path)
        git(tmp_path, "commit", "--allow-empty", "-m", "init")
        if branch != "main":
            git(tmp_path, "checkout", "-b", branch)
        return tmp_path

    return _make


@pytest.fixture
def gate_repo(git_repo: Callable[..., Path]) -> Callable[..., Path]:
    """Factory: ``git_repo`` on a ``feature`` branch, optionally with a note committed on ``main``."""

    def _make(main_note: str | None = None) -> Path:
        repo = git_repo()
        if main_note is not None:
            commit(
                repo, "docs(release-notes): seed", {f"release-notes/{VALID_NOTE_NAME}": main_note}
            )
        git(repo, "checkout", "-b", "feature")
        return repo

    return _make
