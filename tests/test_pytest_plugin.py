"""Tests for `toolr._pytest_plugin`'s repo-root discovery.

Entry-point auto-registration (the `pytest11` wiring in `pyproject.toml`)
is a packaging concern verified by installing the wheel, not by unit
tests here — these exercise the pure discovery function directly.
"""

from __future__ import annotations

import os
import sys
from collections.abc import Callable
from pathlib import Path

import pytest

from toolr.testing._pytest_plugin import _discover_repo_root


@pytest.fixture
def make_tree(tmp_path: Path) -> Callable[..., Path]:
    """Create each relative file path under `tmp_path`; return `tmp_path`.

    Only a `tools/pyproject.toml` counts for discovery, so a bare `tools/` on
    the host above `tmp_path` (e.g. `C:\\tools\\` on Windows runners) can't
    change these results.
    """

    def _make(*files: str) -> Path:
        for rel in files:
            path = tmp_path / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.touch()
        return tmp_path

    return _make


def test_discover_repo_root_finds_project_in_start_dir(make_tree: Callable[..., Path]) -> None:
    root = make_tree("tools/pyproject.toml")
    assert _discover_repo_root(root) == root.resolve()


def test_discover_repo_root_finds_project_in_ancestor(make_tree: Callable[..., Path]) -> None:
    root = make_tree("tools/pyproject.toml", "tools/sub/deeper/.keep")
    assert _discover_repo_root(root / "tools" / "sub" / "deeper") == root.resolve()


def test_discover_repo_root_skips_unrelated_package_tools_dir(
    make_tree: Callable[..., Path],
) -> None:
    root = make_tree("tools/pyproject.toml", "tools/demo.py", "src/pkg/tools/__init__.py")
    assert _discover_repo_root(root / "src" / "pkg") == root.resolve()


def test_discover_repo_root_skips_package_tools_dir_without_pyproject(
    make_tree: Callable[..., Path],
) -> None:
    root = make_tree("tools/pyproject.toml", "proj/tools/__init__.py", "proj/tools/demo.py")
    assert _discover_repo_root(root / "proj") == root.resolve()


def test_discover_repo_root_project_wins_over_bare_ancestor_tools_dir(
    make_tree: Callable[..., Path],
) -> None:
    root = make_tree("tools/.keep", "proj/tools/__init__.py", "proj/tools/pyproject.toml")
    assert _discover_repo_root(root / "proj") == (root / "proj").resolve()


def test_discover_repo_root_returns_none_for_bare_tools_dir(
    make_tree: Callable[..., Path],
) -> None:
    root = make_tree("tools/demo.py", "tools/__init__.py")
    assert _discover_repo_root(root) is None


@pytest.mark.skipif(
    sys.platform == "win32" or os.geteuid() == 0, reason="needs POSIX permissions, not root"
)
def test_discover_repo_root_survives_an_untraversable_tools_dir(
    make_tree: Callable[..., Path],
) -> None:
    root = make_tree("tools/pyproject.toml")
    tools = root / "tools"
    tools.chmod(0)
    try:
        assert _discover_repo_root(root) is None
    finally:
        tools.chmod(0o755)


def test_repo_root_fixture_resolves_to_this_repo(repo_root: Path) -> None:
    # This repo ships its own `tools/` (see `tools/precommit/`), so the
    # plugin's `pytest_configure` hook should have discovered it already.
    assert (repo_root / "tools").is_dir()


def test_repo_root_fixture_fails_without_a_project(pytester: pytest.Pytester) -> None:
    """End-to-end: a real nested pytest run, with no `tools/pyproject.toml` above it.

    Exercises `pytest_configure`'s no-repo-root branch and the
    `repo_root` fixture's failure path for real, rather than mocking
    the plugin's internals. A bare `tools/` doesn't count.
    """
    (pytester.path / "tools").mkdir()
    pytester.makepyfile(
        test_it="""
        def test_uses_repo_root(repo_root):
            pass
        """
    )
    # No `-p` needed: the `pytest11` entry point auto-registers the plugin
    # already, the same way it does for every project depending on toolr-py.
    result = pytester.runpytest()
    # `repo_root` fails during fixture setup, so pytest reports this as an
    # error (not a test failure).
    result.assert_outcomes(errors=1)
    result.stdout.fnmatch_lines(["*toolr: no `tools/pyproject.toml` found above the pytest run*"])
