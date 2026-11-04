"""Tests for ``toolr release-notes check`` and its helpers."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

import pytest

from toolr.testing import make_context
from tools.release_notes import Commit
from tools.release_notes import _file_problems
from tools.release_notes import _has_skip_trailer
from tools.release_notes import _is_breaking
from tools.release_notes import _render
from tools.release_notes import _requires_note
from tools.release_notes import _scan_markdown
from tools.release_notes import check
from tools.release_notes import clear

from tests.tools.release_notes.helpers import FEATURE_NOTE_PATH
from tests.tools.release_notes.helpers import VALID_NOTE_NAME
from tests.tools.release_notes.helpers import commit
from tests.tools.release_notes.helpers import git


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


def _gate_fails(repo: Path, base: str = "main") -> str:
    ctx = make_context(repo)
    with pytest.raises(SystemExit) as exc:
        check(ctx, base=base)
    assert exc.value.code != 0
    return ctx.stderr


def test_gate_feat_without_note_fails(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    commit(repo, "feat(cli): add frobnicate")
    stderr = _gate_fails(repo)
    assert "feat(cli): add frobnicate" in stderr
    assert "toolr release-notes add" in stderr
    assert "Release-Notes: skip" in stderr


def test_gate_feat_with_note_passes(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    commit(repo, "feat: a", {FEATURE_NOTE_PATH: "Added a."})
    check(make_context(repo), base="main")


def test_gate_appended_existing_note_passes(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo(main_note="Old.")
    commit(repo, "fix: a", {f"release-notes/{VALID_NOTE_NAME}": "Old.\n\nNew."})
    check(make_context(repo), base="main")


def test_gate_readme_only_change_fails(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    commit(repo, "feat: a", {"release-notes/README.md": "how"})
    assert "feat: a" in _gate_fails(repo)


def test_gate_breaking_needs_breaking_note(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    commit(repo, "feat!: drop x", {FEATURE_NOTE_PATH: "Dropped x."})
    stderr = _gate_fails(repo)
    assert "feat!: drop x" in stderr
    assert "**Breaking:**" in stderr
    commit(
        repo, "docs(release-notes): mark breaking", {FEATURE_NOTE_PATH: "**Breaking:** Dropped x."}
    )
    check(make_context(repo), base="main")


def test_gate_revert_with_deleted_note_passes(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo(main_note="Added a.")
    commit(repo, "revert: feat: a", {f"release-notes/{VALID_NOTE_NAME}": None})
    check(make_context(repo), base="main")


def test_gate_unconventional_breaking_footer_passes(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    commit(repo, "Some change\n\nBREAKING CHANGE: y")
    check(make_context(repo), base="main")


def test_gate_lowercase_skip_trailer(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    commit(repo, "feat: a\n\nrelease-notes: skip")
    check(make_context(repo), base="main")


def test_gate_chore_only_passes(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    commit(repo, "chore: tidy")
    check(make_context(repo), base="main")


def test_gate_empty_range_fails(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    assert "main" in _gate_fails(repo)


def test_gate_trailer_in_any_commit(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    commit(repo, "feat: a")
    commit(repo, "chore: b\n\nRelease-Notes: skip")
    check(make_context(repo), base="main")


def test_gate_ignores_base_side_commits(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    commit(repo, "chore: branch work")
    git(repo, "checkout", "-q", "main")
    commit(repo, "feat: on main")
    git(repo, "checkout", "-q", "feature")
    check(make_context(repo), base="main")


@pytest.mark.parametrize("with_note", [True, False])
def test_gate_on_pr_merge_commit(gate_repo: Callable[..., Path], with_note: bool) -> None:
    repo = gate_repo()
    commit(repo, "feat: a", {FEATURE_NOTE_PATH: "Added a."} if with_note else None)
    git(repo, "checkout", "-q", "main")
    commit(repo, "chore: unrelated", {"other.txt": "x"})
    base = git(repo, "rev-parse", "HEAD").strip()
    git(repo, "merge", "--no-ff", "-q", "-m", "Merge feature into main", "feature")
    if with_note:
        check(make_context(repo), base=base)
    else:
        assert "feat: a" in _gate_fails(repo, base=base)


def test_gate_unknown_base_fails(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    commit(repo, "chore: x")
    assert "nope" in _gate_fails(repo, base="nope")


def test_gate_runs_file_checks_too(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    commit(repo, "chore: x", {"release-notes/bad.md": "x"})
    assert "bad.md" in _gate_fails(repo)


def test_gate_git_failure_fails(gate_repo: Callable[..., Path]) -> None:
    repo = gate_repo()
    commit(repo, "feat: a", {FEATURE_NOTE_PATH: "Added a."})
    git(repo, "checkout", "-q", "--orphan", "unrelated")
    commit(repo, "chore: unrelated root")
    git(repo, "checkout", "-q", "feature")
    assert "git diff failed" in _gate_fails(repo, base="unrelated")
