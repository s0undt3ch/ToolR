"""Tests for ``toolr release-notes clear``."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

import pytest

from toolr.testing import make_context
from tools.release_notes import clear


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
