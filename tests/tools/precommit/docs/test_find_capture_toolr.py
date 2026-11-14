"""Tests for how ``toolr pre-commit regen-doc-snippets`` picks the toolr binary it captures from."""

from __future__ import annotations

import os
from collections.abc import Callable
from pathlib import Path

import pytest

from tools.precommit.docs import _find_capture_toolr


@pytest.fixture(autouse=True)
def _clean_env(monkeypatch: pytest.MonkeyPatch) -> None:
    """Start without the override, and with no `toolr` on PATH, so each test opts in."""
    monkeypatch.delenv("TOOLR_REGEN_BINARY", raising=False)
    monkeypatch.setenv("PATH", "")


@pytest.fixture
def make_binary(tmp_path: Path) -> Callable[[str, int], Path]:
    """Factory: create `target/<profile>/toolr` under ``tmp_path`` with the given mtime."""

    def factory(profile: str, mtime: int) -> Path:
        binary = tmp_path / "target" / profile / "toolr"
        binary.parent.mkdir(parents=True, exist_ok=True)
        binary.write_text("")
        os.utime(binary, (mtime, mtime))
        return binary

    return factory


def test_env_override_wins_even_when_the_path_is_missing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, make_binary: Callable[[str, int], Path]
) -> None:
    make_binary("debug", 1_000)
    override = tmp_path / "elsewhere" / "toolr"
    monkeypatch.setenv("TOOLR_REGEN_BINARY", str(override))
    assert _find_capture_toolr(tmp_path) == override


@pytest.mark.parametrize(
    ("release_mtime", "debug_mtime", "expected"),
    [(2_000, 1_000, "release"), (1_000, 2_000, "debug")],
)
def test_newest_cargo_build_wins(
    tmp_path: Path,
    make_binary: Callable[[str, int], Path],
    release_mtime: int,
    debug_mtime: int,
    expected: str,
) -> None:
    release = make_binary("release", release_mtime)
    debug = make_binary("debug", debug_mtime)
    assert _find_capture_toolr(tmp_path) == {"release": release, "debug": debug}[expected]


def test_single_cargo_build_is_used(
    tmp_path: Path, make_binary: Callable[[str, int], Path]
) -> None:
    debug = make_binary("debug", 1_000)
    assert _find_capture_toolr(tmp_path) == debug


def test_cargo_build_beats_toolr_on_path(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, make_binary: Callable[[str, int], Path]
) -> None:
    on_path = tmp_path / "bin"
    on_path.mkdir()
    (on_path / "toolr").write_text("")
    (on_path / "toolr").chmod(0o755)
    monkeypatch.setenv("PATH", str(on_path))
    debug = make_binary("debug", 1_000)
    assert _find_capture_toolr(tmp_path) == debug


@pytest.mark.skip_on_windows(reason="relies on the executable bit for PATH lookup")
def test_falls_back_to_toolr_on_path(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    on_path = tmp_path / "bin"
    on_path.mkdir()
    toolr = on_path / "toolr"
    toolr.write_text("")
    toolr.chmod(0o755)
    monkeypatch.setenv("PATH", str(on_path))
    assert _find_capture_toolr(tmp_path / "repo") == toolr


def test_returns_none_when_nothing_is_found(tmp_path: Path) -> None:
    assert _find_capture_toolr(tmp_path) is None
