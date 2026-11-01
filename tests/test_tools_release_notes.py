"""Tests for ``tools/release_notes.py``."""

from __future__ import annotations

import subprocess
from collections.abc import Callable
from datetime import UTC
from datetime import datetime
from pathlib import Path
from typing import Any

import pytest
from tools.release_notes import NOTE_NAME_RE
from tools.release_notes import Commit
from tools.release_notes import _file_problems
from tools.release_notes import _github_env_block
from tools.release_notes import _has_skip_trailer
from tools.release_notes import _is_breaking
from tools.release_notes import _note_filename
from tools.release_notes import _render
from tools.release_notes import _requires_note
from tools.release_notes import _scan_markdown
from tools.release_notes import _slug_from_branch
from tools.release_notes import add
from tools.release_notes import check
from tools.release_notes import clear
from tools.release_notes import render

from toolr.testing import make_context

VALID_NOTE_NAME = "2026-11-01.14-03-27.foo.md"
FEATURE_NOTE_PATH = "release-notes/2026-11-01.15-00-00.bar.md"
LONG_NOTE = (
    "Fixed `Optional[bool]` handling; see [link](url) and ~/src/[wip]/ for details. "
    + "word " * 30
    + "end :smile:"
)


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


@pytest.mark.parametrize(
    "files", [{}, {"README.md": "how to"}, None], ids=["empty-dir", "only-readme", "missing-dir"]
)
def test_render_empty(
    tmp_path: Path, notes_dir: Callable[[dict[str, str]], Path], files: dict[str, str] | None
) -> None:
    directory = notes_dir(files) if files is not None else tmp_path / "release-notes"
    assert _render(directory) == ""


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


@pytest.mark.parametrize(
    "note",
    ["A", LONG_NOTE, "Makefile:\n\n```make\nall:\n\techo hi\n```"],
    ids=["plain", "verbatim-long-line", "keeps-tabs"],
)
def test_render_command_stdout(
    tmp_path: Path,
    notes_dir: Callable[[dict[str, str]], Path],
    capsys: pytest.CaptureFixture[str],
    note: str,
) -> None:
    notes_dir({"2026-01-01.00-00-00.a.md": note})
    render(make_context(repo_root=tmp_path))
    assert capsys.readouterr().out == note + "\n"


def test_render_command_github_env(
    tmp_path: Path, notes_dir: Callable[[dict[str, str]], Path], capsys: pytest.CaptureFixture[str]
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
    assert capsys.readouterr().out == ""


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


def test_valid_tree_no_problems(
    notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path
) -> None:
    notes_dir({"README.md": "how", VALID_NOTE_NAME: "Text\n\n```sh\n# comment\n```\n"})
    assert _file_problems(tmp_path) == []


def test_missing_dir_no_problems(tmp_path: Path) -> None:
    assert _file_problems(tmp_path) == []


@pytest.mark.parametrize(
    "name",
    [
        "foo.md",
        "2026-11-01-foo.md",
        "2026-11-01.14-03-27.Foo.md",
        "2026-02-30.10-00-00.x.md",
        "2026-01-01.25-00-00.x.md",
    ],
)
def test_bad_name_or_date(
    notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path, name: str
) -> None:
    notes_dir({name: "Text"})
    (problem,) = _file_problems(tmp_path)
    assert name in problem


@pytest.mark.parametrize("content", ["", "  \n"])
def test_empty_note(
    notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path, content: str
) -> None:
    notes_dir({VALID_NOTE_NAME: content})
    (problem,) = _file_problems(tmp_path)
    assert VALID_NOTE_NAME in problem
    assert "empty" in problem


def test_heading_flagged(notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path) -> None:
    notes_dir({VALID_NOTE_NAME: "Text\n\n## Heading"})
    (problem,) = _file_problems(tmp_path)
    assert VALID_NOTE_NAME in problem
    assert "heading" in problem


@pytest.mark.parametrize("fence", ["```", "~~~"])
def test_heading_inside_fence_ok(
    notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path, fence: str
) -> None:
    notes_dir({VALID_NOTE_NAME: f"{fence}sh\n# comment\n{fence}"})
    assert _file_problems(tmp_path) == []


def test_unreleased_md_flagged(tmp_path: Path) -> None:
    (tmp_path / "UNRELEASED.md").write_text("old")
    (problem,) = _file_problems(tmp_path)
    assert "UNRELEASED.md" in problem
    assert "toolr release-notes add" in problem


def test_check_command_exit(notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path) -> None:
    directory = notes_dir({VALID_NOTE_NAME: "Text"})
    check(make_context(tmp_path))
    (directory / "foo.md").write_text("x")
    (directory / "2026-11-01.14-03-28.bar.md").write_text("")
    ctx = make_context(tmp_path)
    with pytest.raises(SystemExit) as exc:
        check(ctx)
    assert exc.value.code != 0
    assert "foo.md" in ctx.stderr
    assert "2026-11-01.14-03-28.bar.md" in ctx.stderr


def test_render_refuses_invalid_tree(
    notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    notes_dir({VALID_NOTE_NAME: ""})
    ctx = make_context(tmp_path)
    with pytest.raises(SystemExit) as exc:
        render(ctx)
    assert exc.value.code != 0
    assert ctx.stdout == ""
    assert VALID_NOTE_NAME in ctx.stderr
    assert capsys.readouterr().out == ""


@pytest.mark.parametrize(
    ("content", "has_heading", "open_fence"),
    [
        ("````\n```\n## Heading\n```\n````", False, False),
        ("```\n~~~\n# x\n```", False, False),
        ("```\n```sh\n# x\n```", False, False),
        ("Text\n    ```\n# x", True, False),
        ("```\ncode\n```\n# x", True, False),
        ("```\n# unclosed", False, True),
        ("~~~\n# x\n~~~\n## y", True, False),
        ("``` a`b\n# x", True, False),
        ("#hashtag", False, False),
        ("#!/bin/sh", False, False),
        ("Text\n\n  ## x", True, False),
        ("Text\n\n   # x", True, False),
        ("Text\n\n    # x", False, False),
        ("Intro paragraph\n---\nmore", True, False),
        ("Title\n===", True, False),
        ("Para\n\n---\n\nMore", False, False),
        ("- item\n---", False, False),
        ("```\ntext\n---\n```", False, False),
        ("```toml\nx = 1", False, True),
        ("~~~\nx", False, True),
        ("````\n```\nx\n```", False, True),
        ("```toml\nx = 1\n```", False, False),
        ("Text\n\n~~~\nx\n~~~\n", False, False),
    ],
)
def test_scan_markdown(content: str, has_heading: bool, open_fence: bool) -> None:
    assert _scan_markdown(content) == (has_heading, open_fence)


def test_unclosed_fence_flagged(
    notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path
) -> None:
    notes_dir({VALID_NOTE_NAME: "```toml\nx = 1"})
    (problem,) = _file_problems(tmp_path)
    assert VALID_NOTE_NAME in problem
    assert "unclosed code fence" in problem


@pytest.mark.parametrize("name", [".DS_Store", ".x.md.swp", "x.md~", "notes.txt", "sub/x.md"])
def test_non_note_entries_ignored(
    notes_dir: Callable[[dict[str, str]], Path], tmp_path: Path, name: str
) -> None:
    directory = notes_dir({VALID_NOTE_NAME: "Text", name: "junk"})
    assert _file_problems(tmp_path) == []
    assert _render(directory) == "Text"
    clear(make_context(tmp_path))
    assert [p.name for p in directory.iterdir()] == [Path(name).parts[0]]


def test_invalid_utf8_flagged(tmp_path: Path) -> None:
    directory = tmp_path / "release-notes"
    directory.mkdir()
    (directory / VALID_NOTE_NAME).write_bytes(b"\xff\xfe")
    (problem,) = _file_problems(tmp_path)
    assert "not valid UTF-8" in problem


@pytest.mark.parametrize(
    ("subject", "body", "required"),
    [
        ("feat: x", "", True),
        ("fix(cli): x", "", True),
        ("revert: x", "", True),
        ("refactor!: x", "", True),
        ("refactor: x", "BREAKING CHANGE: y", True),
        ("fix(deps): x", "", False),
        ("fix(deps)!: x", "", True),
        ("chore: x", "", False),
        ("docs(release-notes): x", "", False),
        ("fix: x", "Changelog: skip", False),
        ('Revert "feat: x"', "", False),
        ("Some change", "BREAKING CHANGE: y", False),
        ("Merge branch 'x'", "", False),
    ],
)
def test_requires_note(subject: str, body: str, required: bool) -> None:
    assert _requires_note(Commit("0" * 40, subject, body)) is required


@pytest.mark.parametrize(
    ("subject", "body", "breaking"),
    [
        ("feat!: x", "", True),
        ("refactor: x", "BREAKING CHANGE: y", True),
        ("feat: x", "", False),
        ("Some change", "BREAKING CHANGE: y", False),
        ("Some change!: x", "", False),
    ],
)
def test_is_breaking(subject: str, body: str, breaking: bool) -> None:
    assert _is_breaking(Commit("0" * 40, subject, body)) is breaking


@pytest.mark.parametrize(
    ("body", "skip"),
    [
        ("Release-Notes: skip", True),
        ("release-notes: skip", True),
        ("RELEASE-NOTES:\tSKIP", True),
        ("x\n\nRelease-Notes: skip  \n", True),
        ("Release-Notes:\n\nskip", False),
        ("Release-Notes: skip\nnope", True),
        ("Release-Notes: skipped", False),
    ],
)
def test_has_skip_trailer(body: str, skip: bool) -> None:
    assert _has_skip_trailer(body) is skip


@pytest.fixture
def gate_repo(git_repo: Callable[..., Path]) -> Callable[..., Path]:
    """Factory: ``git_repo`` on a ``feature`` branch, optionally with a note committed on ``main``."""

    def _make(main_note: str | None = None) -> Path:
        repo = git_repo()
        if main_note is not None:
            _commit(
                repo, "docs(release-notes): seed", {f"release-notes/{VALID_NOTE_NAME}": main_note}
            )
        _git(repo, "checkout", "-b", "feature")
        return repo

    return _make


def _commit(repo: Path, message: str, files: dict[str, str | None] | None = None) -> None:
    for name, text in (files or {}).items():
        path = repo / name
        if text is None:
            _git(repo, "rm", "-q", "--", name)
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        _git(repo, "add", "--", name)
    _git(repo, "commit", "--allow-empty", "-q", "-m", message)


def _gate_fails(repo: Path, base: str = "main") -> str:
    ctx = make_context(repo)
    with pytest.raises(SystemExit) as exc:
        check(ctx, base=base)
    assert exc.value.code != 0
    return ctx.stderr


def test_gate_feat_without_note_fails(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    _commit(repo, "feat(cli): add frobnicate")
    stderr = _gate_fails(repo)
    assert "feat(cli): add frobnicate" in stderr
    assert "toolr release-notes add" in stderr
    assert "Release-Notes: skip" in stderr


def test_gate_feat_with_note_passes(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    _commit(repo, "feat: a", {FEATURE_NOTE_PATH: "Added a."})
    check(make_context(repo), base="main")


def test_gate_appended_existing_note_passes(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo(main_note="Old.")
    _commit(repo, "fix: a", {f"release-notes/{VALID_NOTE_NAME}": "Old.\n\nNew."})
    check(make_context(repo), base="main")


def test_gate_readme_only_change_fails(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    _commit(repo, "feat: a", {"release-notes/README.md": "how"})
    assert "feat: a" in _gate_fails(repo)


def test_gate_breaking_needs_breaking_note(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    _commit(repo, "feat!: drop x", {FEATURE_NOTE_PATH: "Dropped x."})
    stderr = _gate_fails(repo)
    assert "feat!: drop x" in stderr
    assert "**Breaking:**" in stderr
    _commit(
        repo, "docs(release-notes): mark breaking", {FEATURE_NOTE_PATH: "**Breaking:** Dropped x."}
    )
    check(make_context(repo), base="main")


def test_gate_revert_with_deleted_note_passes(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo(main_note="Added a.")
    _commit(repo, "revert: feat: a", {f"release-notes/{VALID_NOTE_NAME}": None})
    check(make_context(repo), base="main")


def test_gate_unconventional_breaking_footer_passes(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    _commit(repo, "Some change\n\nBREAKING CHANGE: y")
    check(make_context(repo), base="main")


def test_gate_lowercase_skip_trailer(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    _commit(repo, "feat: a\n\nrelease-notes: skip")
    check(make_context(repo), base="main")


def test_gate_chore_only_passes(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    _commit(repo, "chore: tidy")
    check(make_context(repo), base="main")


def test_gate_empty_range_fails(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    assert "main" in _gate_fails(repo)


def test_gate_trailer_in_any_commit(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    _commit(repo, "feat: a")
    _commit(repo, "chore: b\n\nRelease-Notes: skip")
    check(make_context(repo), base="main")


def test_gate_ignores_base_side_commits(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    _commit(repo, "chore: branch work")
    _git(repo, "checkout", "-q", "main")
    _commit(repo, "feat: on main")
    _git(repo, "checkout", "-q", "feature")
    check(make_context(repo), base="main")


@pytest.mark.parametrize("with_note", [True, False])
def test_gate_on_pr_merge_commit(gate_repo: Callable[..., Path], with_note: bool) -> None:
    repo = gate_repo()
    _commit(repo, "feat: a", {FEATURE_NOTE_PATH: "Added a."} if with_note else None)
    _git(repo, "checkout", "-q", "main")
    _commit(repo, "chore: unrelated", {"other.txt": "x"})
    base = _git(repo, "rev-parse", "HEAD").strip()
    _git(repo, "merge", "--no-ff", "-q", "-m", "Merge feature into main", "feature")
    if with_note:
        check(make_context(repo), base=base)
    else:
        assert "feat: a" in _gate_fails(repo, base=base)


def test_gate_unknown_base_fails(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    _commit(repo, "chore: x")
    assert "nope" in _gate_fails(repo, base="nope")


def test_gate_runs_file_checks_too(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    _commit(repo, "chore: x", {"release-notes/bad.md": "x"})
    assert "bad.md" in _gate_fails(repo)


def test_gate_git_failure_fails(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    _commit(repo, "feat: a", {FEATURE_NOTE_PATH: "Added a."})
    _git(repo, "checkout", "-q", "--orphan", "unrelated")
    _commit(repo, "chore: unrelated root")
    _git(repo, "checkout", "-q", "feature")
    assert "git diff failed" in _gate_fails(repo, base="unrelated")
