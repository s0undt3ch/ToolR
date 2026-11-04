"""Tests for ``toolr release-notes render``."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

import pytest

from toolr.testing import make_context

from tools.release_notes import _github_env_block
from tools.release_notes import _render
from tools.release_notes import render

from tests.tools.release_notes.helpers import LONG_NOTE
from tests.tools.release_notes.helpers import VALID_NOTE_NAME


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
