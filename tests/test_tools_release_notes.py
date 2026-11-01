"""Tests for ``tools/release_notes.py``."""

from __future__ import annotations

import secrets
from collections.abc import Callable
from pathlib import Path

import pytest
from tools.release_notes import _github_env_block
from tools.release_notes import _render
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
