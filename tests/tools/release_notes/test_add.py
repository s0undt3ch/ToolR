"""Tests for ``toolr release-notes add``."""

from __future__ import annotations

from collections.abc import Callable
from datetime import UTC
from datetime import datetime
from pathlib import Path
from typing import Any

import pytest

from toolr.testing import make_context
from tools.release_notes import NOTE_NAME_RE
from tools.release_notes import _note_filename
from tools.release_notes import _slug_from_branch
from tools.release_notes import add

from tests._git import git
from tests.tools.release_notes.helpers import notes
from tests.tools.release_notes.helpers import staged


@pytest.mark.parametrize(
    ("branch", "slug"),
    [
        ("fix/ctx-run-input-deadlock", "ctx-run-input-deadlock"),
        ("feat/Repo_Trust", "repo-trust"),
        ("release-notes-fragments", "release-notes-fragments"),
    ],
)
def test_slug_from_branch(branch: str, slug: str) -> None:
    assert _slug_from_branch(branch) == slug


@pytest.mark.parametrize("branch", ["main", "fix/", "fix/___"])
def test_slug_from_branch_rejects_unusable(branch: str) -> None:
    assert _slug_from_branch(branch) is None


def test_note_filename() -> None:
    now = datetime(2026, 11, 1, 14, 3, 27, tzinfo=UTC)
    assert _note_filename(now, "x") == "2026-11-01.14-03-27.x.md"


def test_add_writes_and_stages(git_repo: Callable[..., Path]) -> None:
    repo = git_repo()
    ctx = make_context(repo)
    add(ctx, slug="foo", text="Hello")
    (note,) = notes(repo)
    match = NOTE_NAME_RE.match(note.name)
    assert match
    assert match["slug"] == "foo"
    assert note.read_text() == "Hello\n"
    assert staged(repo) == [f"release-notes/{note.name}"]
    assert f"release-notes/{note.name}" in ctx.stdout


def test_add_without_text_creates_empty_file(git_repo: Callable[..., Path]) -> None:
    repo = git_repo()
    add(make_context(repo), slug="foo")
    (note,) = notes(repo)
    assert note.read_text() == ""
    assert staged(repo) == [f"release-notes/{note.name}"]


def test_add_derives_slug_from_branch(git_repo: Callable[..., Path]) -> None:
    repo = git_repo("fix/some-thing")
    add(make_context(repo), text="x")
    (note,) = notes(repo)
    match = NOTE_NAME_RE.match(note.name)
    assert match
    assert match["slug"] == "some-thing"


def test_add_detached_head_fails(git_repo: Callable[..., Path]) -> None:
    repo = git_repo()
    git(repo, "checkout", "--detach")
    ctx = make_context(repo)
    with pytest.raises(SystemExit) as exc:
        add(ctx, text="x")
    assert exc.value.code != 0
    assert not (repo / "release-notes").exists()


@pytest.mark.parametrize(
    "kwargs",
    [
        {"text": "x"},
        {"slug": "foo", "append": True},
        {"slug": "foo", "text": "B", "append": True},
        {"slug": "Bad Slug", "text": "x"},
    ],
    ids=["on-main-without-slug", "append-without-text", "append-without-note", "invalid-slug"],
)
def test_add_fails(git_repo: Callable[..., Path], kwargs: dict[str, Any]) -> None:
    repo = git_repo()
    with pytest.raises(SystemExit) as exc:
        add(make_context(repo), **kwargs)
    assert exc.value.code != 0
    assert not (repo / "release-notes").exists()


def test_add_outside_git_repo_reports_git_failure(tmp_path: Path) -> None:
    ctx = make_context(tmp_path)
    with pytest.raises(SystemExit) as exc:
        add(ctx, text="x")
    assert exc.value.code != 0
    assert "git branch failed" in ctx.stderr


def test_add_duplicate_slug_refused(git_repo: Callable[..., Path]) -> None:
    repo = git_repo()
    directory = repo / "release-notes"
    directory.mkdir()
    (directory / "2020-01-01.00-00-00.foo.md").write_text("A\n")
    ctx = make_context(repo)
    with pytest.raises(SystemExit) as exc:
        add(ctx, slug="foo", text="B")
    assert exc.value.code != 0
    assert "foo" in ctx.stderr
    assert [p.name for p in notes(repo)] == ["2020-01-01.00-00-00.foo.md"]


def test_add_append(git_repo: Callable[..., Path]) -> None:
    repo = git_repo()
    directory = repo / "release-notes"
    directory.mkdir()
    existing = directory / "2020-01-01.00-00-00.foo.md"
    existing.write_text("A")
    add(make_context(repo), slug="foo", text="B", append=True)
    assert [p.name for p in notes(repo)] == [existing.name]
    assert existing.read_text() == "A\n\nB\n"
    assert staged(repo) == [f"release-notes/{existing.name}"]
