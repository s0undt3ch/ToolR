"""Tests for ``tools/release_notes.py``."""

from __future__ import annotations

import secrets
import subprocess
from collections.abc import Callable
from datetime import UTC
from datetime import datetime
from pathlib import Path

import pytest
from tools.release_notes import NOTE_NAME_RE
from tools.release_notes import _file_problems
from tools.release_notes import _github_env_block
from tools.release_notes import _note_filename
from tools.release_notes import _render
from tools.release_notes import _slug_from_branch
from tools.release_notes import add
from tools.release_notes import check
from tools.release_notes import clear
from tools.release_notes import render

from toolr.testing import make_context


@pytest.fixture
def notes_dir(tmp_path: Path) -> Callable[[dict[str, str]], Path]:
    """Factory: write ``files`` into ``tmp_path/release-notes``. Returns the directory."""

    def _make(files: dict[str, str]) -> Path:
        directory = tmp_path / "release-notes"
        directory.mkdir()
        for name, text in files.items():
            (directory / name).write_bytes(text.encode("utf-8"))
        return directory

    return _make


def test_render_empty_dir(notes_dir: Callable[[dict[str, str]], Path]) -> None:
    assert _render(notes_dir({})) == ""


def test_render_only_readme(notes_dir: Callable[[dict[str, str]], Path]) -> None:
    assert _render(notes_dir({"README.md": "how to"})) == ""


def test_render_missing_dir(tmp_path: Path) -> None:
    assert _render(tmp_path / "release-notes") == ""


def test_render_excludes_readme(notes_dir: Callable[[dict[str, str]], Path]) -> None:
    out = _render(notes_dir({"README.md": "READ ME", "2026-01-01.00-00-00.a.md": "A"}))
    assert "READ ME" not in out
    assert out == "A"


def test_render_breaking_first(notes_dir: Callable[[dict[str, str]], Path]) -> None:
    directory = notes_dir(
        {
            "2026-01-01.00-00-00.a.md": "A",
            "2026-01-02.00-00-00.b.md": "**Breaking:** B",
            "2026-01-03.00-00-00.c.md": "C",
        }
    )
    assert _render(directory) == "**Breaking:** B\n\nA\n\nC"


def test_render_normalises_crlf(notes_dir: Callable[[dict[str, str]], Path]) -> None:
    directory = notes_dir({"2026-01-01.00-00-00.a.md": "One\r\n\r\nTwo\r\n\r\n"})
    assert _render(directory) == "One\n\nTwo"


def test_github_env_block_format() -> None:
    block = _github_env_block("TOOLR_RELEASE_NOTES", "value")
    head, body, delimiter, tail = block.split("\n")
    assert head.startswith("TOOLR_RELEASE_NOTES<<TOOLR_RELEASE_NOTES_")
    assert delimiter == head.split("<<", 1)[1]
    assert (body, tail) == ("value", "")
    assert block == f"TOOLR_RELEASE_NOTES<<{delimiter}\nvalue\n{delimiter}\n"


def test_github_env_block_avoids_delimiter_in_value(monkeypatch: pytest.MonkeyPatch) -> None:
    tokens = iter(["X", "Y"])
    monkeypatch.setattr(secrets, "token_hex", lambda _n: next(tokens))
    block = _github_env_block("TOOLR_RELEASE_NOTES", "TOOLR_RELEASE_NOTES_X")
    assert block == (
        "TOOLR_RELEASE_NOTES<<TOOLR_RELEASE_NOTES_Y\nTOOLR_RELEASE_NOTES_X\nTOOLR_RELEASE_NOTES_Y\n"
    )


def test_render_command_stdout(tmp_path: Path, notes_dir: Callable[[dict[str, str]], Path]) -> None:
    notes_dir({"2026-01-01.00-00-00.a.md": "A"})
    ctx = make_context(repo_root=tmp_path)
    render(ctx)
    assert ctx.stdout == "A\n"


def test_render_command_github_env(
    tmp_path: Path, notes_dir: Callable[[dict[str, str]], Path]
) -> None:
    notes_dir({"2026-01-01.00-00-00.a.md": "A"})
    env_file = tmp_path / "github_env"
    env_file.write_text("EXISTING=1\n", encoding="utf-8")
    ctx = make_context(repo_root=tmp_path)
    render(ctx, github_env=str(env_file))
    content = env_file.read_text(encoding="utf-8")
    assert content.startswith("EXISTING=1\nTOOLR_RELEASE_NOTES<<TOOLR_RELEASE_NOTES_")
    assert content.endswith("\nA\n" + content.splitlines()[1].split("<<", 1)[1] + "\n")
    assert ctx.stdout == ""


def _git(repo: Path, *args: str) -> str:
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


@pytest.fixture
def git_repo(tmp_path: Path) -> Callable[..., Path]:
    """Factory: a repo with one commit on ``main``, checked out to ``branch``."""

    def _make(branch: str = "main") -> Path:
        _git(tmp_path, "init", "-b", "main")
        _git(tmp_path, "config", "user.name", "Test")
        _git(tmp_path, "config", "user.email", "test@example.com")
        _git(tmp_path, "config", "commit.gpgsign", "false")
        _git(tmp_path, "commit", "--allow-empty", "-m", "init")
        if branch != "main":
            _git(tmp_path, "checkout", "-b", branch)
        return tmp_path

    return _make


def _staged(repo: Path) -> list[str]:
    return _git(repo, "diff", "--cached", "--name-only").split()


def _notes(repo: Path) -> list[Path]:
    return sorted((repo / "release-notes").glob("*.md"))


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


@pytest.mark.parametrize("branch", ["HEAD", "main", "fix/", "fix/___"])
def test_slug_from_branch_rejects_unusable(branch: str) -> None:
    assert _slug_from_branch(branch) is None


def test_note_filename() -> None:
    now = datetime(2026, 11, 1, 14, 3, 27, tzinfo=UTC)
    assert _note_filename(now, "x") == "2026-11-01.14-03-27.x.md"


def test_add_writes_and_stages(git_repo: Callable[..., Path]) -> None:
    repo = git_repo()
    ctx = make_context(repo)
    add(ctx, slug="foo", text="Hello")
    (note,) = _notes(repo)
    match = NOTE_NAME_RE.match(note.name)
    assert match
    assert match["slug"] == "foo"
    assert note.read_text() == "Hello\n"
    assert _staged(repo) == [f"release-notes/{note.name}"]
    assert f"release-notes/{note.name}" in ctx.stdout


def test_add_without_text_creates_empty_file(git_repo: Callable[..., Path]) -> None:
    repo = git_repo()
    add(make_context(repo), slug="foo")
    (note,) = _notes(repo)
    assert note.read_text() == ""
    assert _staged(repo) == [f"release-notes/{note.name}"]


def test_add_derives_slug_from_branch(git_repo: Callable[..., Path]) -> None:
    repo = git_repo("fix/some-thing")
    add(make_context(repo), text="x")
    (note,) = _notes(repo)
    match = NOTE_NAME_RE.match(note.name)
    assert match
    assert match["slug"] == "some-thing"


def test_add_detached_head_fails(git_repo: Callable[..., Path]) -> None:
    repo = git_repo()
    _git(repo, "checkout", "--detach")
    ctx = make_context(repo)
    with pytest.raises(SystemExit) as exc:
        add(ctx, text="x")
    assert exc.value.code != 0
    assert not (repo / "release-notes").exists()


def test_add_on_main_without_slug_fails(git_repo: Callable[..., Path]) -> None:
    repo = git_repo()
    with pytest.raises(SystemExit) as exc:
        add(make_context(repo), text="x")
    assert exc.value.code != 0
    assert not (repo / "release-notes").exists()


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
    assert [p.name for p in _notes(repo)] == ["2020-01-01.00-00-00.foo.md"]


def test_add_append(git_repo: Callable[..., Path]) -> None:
    repo = git_repo()
    directory = repo / "release-notes"
    directory.mkdir()
    existing = directory / "2020-01-01.00-00-00.foo.md"
    existing.write_text("A")
    add(make_context(repo), slug="foo", text="B", append=True)
    assert [p.name for p in _notes(repo)] == [existing.name]
    assert existing.read_text() == "A\n\nB\n"
    assert _staged(repo) == [f"release-notes/{existing.name}"]


def test_add_append_without_text_fails(git_repo: Callable[..., Path]) -> None:
    repo = git_repo()
    with pytest.raises(SystemExit) as exc:
        add(make_context(repo), slug="foo", append=True)
    assert exc.value.code != 0
    assert not (repo / "release-notes").exists()


def test_add_append_without_existing_note_fails(git_repo: Callable[..., Path]) -> None:
    repo = git_repo()
    with pytest.raises(SystemExit) as exc:
        add(make_context(repo), slug="foo", text="B", append=True)
    assert exc.value.code != 0
    assert not (repo / "release-notes").exists()


def test_add_rejects_invalid_slug(git_repo: Callable[..., Path]) -> None:
    repo = git_repo()
    with pytest.raises(SystemExit) as exc:
        add(make_context(repo), slug="Bad Slug", text="x")
    assert exc.value.code != 0
    assert not (repo / "release-notes").exists()


def test_clear_keeps_readme(notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path) -> None:
    directory = notes_dir(
        {"README.md": "how", "2026-01-01.00-00-00.a.md": "A", "2026-01-02.00-00-00.b.md": "B"}
    )
    clear(make_context(tmp_path))
    assert [p.name for p in directory.iterdir()] == ["README.md"]


def test_clear_empty_ok(notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path) -> None:
    directory = notes_dir({"README.md": "how"})
    clear(make_context(tmp_path))
    assert [p.name for p in directory.iterdir()] == ["README.md"]


def test_clear_missing_dir_ok(tmp_path: Path) -> None:
    clear(make_context(tmp_path))
    assert not (tmp_path / "release-notes").exists()


def test_clear_rendered_guard(notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path) -> None:
    directory = notes_dir({"2026-01-01.00-00-00.a.md": "A"})
    rendered = tmp_path / "rendered.md"
    rendered.write_text("## v1\n\n- a commit\n")
    ctx = make_context(tmp_path)
    with pytest.raises(SystemExit) as exc:
        clear(ctx, rendered=str(rendered))
    assert exc.value.code != 0
    assert (directory / "2026-01-01.00-00-00.a.md").exists()

    rendered.write_text("## v1\n\n  ### Notes  \nA\n")
    clear(make_context(tmp_path), rendered=str(rendered))
    assert not (directory / "2026-01-01.00-00-00.a.md").exists()


def test_clear_rendered_guard_no_notes(
    notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path
) -> None:
    notes_dir({"README.md": "how"})
    rendered = tmp_path / "rendered.md"
    rendered.write_text("## v1\n")
    clear(make_context(tmp_path), rendered=str(rendered))


GOOD = "2026-11-01.14-03-27.foo.md"


def test_valid_tree_no_problems(
    notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path
) -> None:
    notes_dir({"README.md": "how", GOOD: "Text\n\n```sh\n# comment\n```\n"})
    assert _file_problems(tmp_path) == []


def test_missing_dir_no_problems(tmp_path: Path) -> None:
    assert _file_problems(tmp_path) == []


@pytest.mark.parametrize("name", ["foo.md", "2026-11-01-foo.md", "2026-11-01.14-03-27.Foo.md"])
def test_bad_name(notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path, name: str) -> None:
    notes_dir({name: "Text"})
    (problem,) = _file_problems(tmp_path)
    assert name in problem


@pytest.mark.parametrize("name", ["2026-02-30.10-00-00.x.md", "2026-01-01.25-00-00.x.md"])
def test_impossible_date(
    notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path, name: str
) -> None:
    notes_dir({name: "Text"})
    (problem,) = _file_problems(tmp_path)
    assert name in problem


def test_subdirectory_flagged(notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path) -> None:
    directory = notes_dir({})
    (directory / "sub").mkdir()
    (problem,) = _file_problems(tmp_path)
    assert "sub" in problem


@pytest.mark.parametrize("content", ["", "  \n"])
def test_empty_note(
    notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path, content: str
) -> None:
    notes_dir({GOOD: content})
    (problem,) = _file_problems(tmp_path)
    assert GOOD in problem
    assert "empty" in problem


def test_heading_flagged(notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path) -> None:
    notes_dir({GOOD: "Text\n\n## Heading"})
    (problem,) = _file_problems(tmp_path)
    assert GOOD in problem
    assert "heading" in problem


@pytest.mark.parametrize("fence", ["```", "~~~"])
def test_heading_inside_fence_ok(
    notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path, fence: str
) -> None:
    notes_dir({GOOD: f"{fence}sh\n# comment\n{fence}"})
    assert _file_problems(tmp_path) == []


def test_unreleased_md_flagged(tmp_path: Path) -> None:
    (tmp_path / "UNRELEASED.md").write_text("old")
    (problem,) = _file_problems(tmp_path)
    assert "UNRELEASED.md" in problem
    assert "toolr release-notes add" in problem


def test_check_command_exit(notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path) -> None:
    directory = notes_dir({GOOD: "Text"})
    check(make_context(tmp_path))
    (directory / "foo.md").write_text("x")
    (directory / "2026-11-01.14-03-28.bar.md").write_text("")
    ctx = make_context(tmp_path)
    with pytest.raises(SystemExit) as exc:
        check(ctx)
    assert exc.value.code != 0
    assert "foo.md" in ctx.stderr
    assert "2026-11-01.14-03-28.bar.md" in ctx.stderr


def test_check_accepts_base(notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path) -> None:
    notes_dir({GOOD: "Text"})
    check(make_context(tmp_path), base="origin/main")


def test_render_refuses_invalid_tree(
    notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path
) -> None:
    notes_dir({GOOD: ""})
    ctx = make_context(tmp_path)
    with pytest.raises(SystemExit) as exc:
        render(ctx)
    assert exc.value.code != 0
    assert ctx.stdout == ""
    assert GOOD in ctx.stderr


@pytest.mark.parametrize(
    ("content", "flagged"),
    [
        ("````\n```\n## Heading\n```\n````", False),
        ("```\n~~~\n# x\n```", False),
        ("```\n```sh\n# x\n```", False),
        ("Text\n    ```\n# x", True),
        ("```\ncode\n```\n# x", True),
        ("```\n# unclosed", False),
        ("~~~\n# x\n~~~\n## y", True),
        ("``` a`b\n# x", True),
        ("#hashtag", False),
        ("#!/bin/sh", False),
    ],
)
def test_fence_tracking(
    notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path, content: str, flagged: bool
) -> None:
    notes_dir({GOOD: content})
    assert bool(_file_problems(tmp_path)) is flagged


def test_invalid_utf8_flagged(tmp_path: Path) -> None:
    directory = tmp_path / "release-notes"
    directory.mkdir()
    (directory / GOOD).write_bytes(b"\xff\xfe")
    (problem,) = _file_problems(tmp_path)
    assert "not valid UTF-8" in problem
