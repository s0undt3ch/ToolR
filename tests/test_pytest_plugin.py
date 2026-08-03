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


def test_discover_repo_root_returns_none_when_no_tools_dir(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    # Confine the ancestor walk to tmp_path: real machines can have a
    # `tools/` dir above it (e.g. GitHub's Windows runners ship `C:\tools\`),
    # which would otherwise make this test flaky depending on the host.
    real_is_dir = Path.is_dir
    boundary = tmp_path.resolve()

    def fake_is_dir(self: Path) -> bool:
        try:
            self.relative_to(boundary)
        except ValueError:
            return False
        return real_is_dir(self)

    monkeypatch.setattr(Path, "is_dir", fake_is_dir)
    assert _discover_repo_root(tmp_path) is None


def test_repo_root_fixture_resolves_to_this_repo(repo_root: Path) -> None:
    # This repo ships its own `tools/` (see `tools/precommit/`), so the
    # plugin's `pytest_configure` hook should have discovered it already.
    assert (repo_root / "tools").is_dir()


def test_repo_root_fixture_fails_without_a_tools_dir(
    pytester: pytest.Pytester, monkeypatch: pytest.MonkeyPatch
) -> None:
    """End-to-end: a real nested pytest run, with no `tools/` above it.

    Exercises `pytest_configure`'s no-repo-root branch and the
    `repo_root` fixture's failure path for real, rather than mocking
    the plugin's internals. Confines the ancestor walk to `pytester.path`
    for the same reason as `test_discover_repo_root_returns_none_when_no_tools_dir`
    (real machines can have a `tools/` dir above it).
    """
    real_is_dir = Path.is_dir
    boundary = pytester.path.resolve()

    def fake_is_dir(self: Path) -> bool:
        try:
            self.relative_to(boundary)
        except ValueError:
            return False
        return real_is_dir(self)

    monkeypatch.setattr(Path, "is_dir", fake_is_dir)

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
    result.stdout.fnmatch_lines(["*toolr: no `tools/` directory found above the pytest run*"])
