"""Tests for `toolr._pytest_plugin`'s repo-root discovery.

Entry-point auto-registration (the `pytest11` wiring in `pyproject.toml`)
is a packaging concern verified by installing the wheel, not by unit
tests here — these exercise the pure discovery function directly.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from toolr.testing._pytest_plugin import _discover_repo_root


@pytest.fixture
def repo_with_tools(tmp_path: Path) -> Path:
    (tmp_path / "tools").mkdir()
    return tmp_path


def test_discover_repo_root_finds_tools_in_start_dir(repo_with_tools: Path) -> None:
    assert _discover_repo_root(repo_with_tools) == repo_with_tools.resolve()


def test_discover_repo_root_finds_tools_in_ancestor(repo_with_tools: Path) -> None:
    nested = repo_with_tools / "tools" / "sub" / "deeper"
    nested.mkdir(parents=True)
    assert _discover_repo_root(nested) == repo_with_tools.resolve()


def test_discover_repo_root_returns_none_when_no_tools_dir(tmp_path: Path) -> None:
    # tmp_path itself has no `tools/`, and (barring a coincidental `tools/`
    # somewhere above the pytest tmp dir root) neither do its ancestors.
    assert _discover_repo_root(tmp_path) is None


def test_repo_root_fixture_resolves_to_this_repo(repo_root: Path) -> None:
    # This repo ships its own `tools/` (see `tools/precommit/`), so the
    # plugin's `pytest_configure` hook should have discovered it already.
    assert (repo_root / "tools").is_dir()
