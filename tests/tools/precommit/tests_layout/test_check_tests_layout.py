"""Tests for ``toolr pre-commit check-tests-layout``."""

from __future__ import annotations

import shutil
from collections.abc import Callable
from pathlib import Path

import pytest

from toolr.testing import make_context

from tools.precommit.tests_layout import check_tests_layout
from tools.precommit.tests_layout import layout_problems

from tests._paths import REPO_ROOT
from tests.tools.precommit.recording_run import RecordingRun


@pytest.fixture
def make_tree(tmp_path: Path) -> Callable[..., Path]:
    """Factory: touch each relative path under ``tmp_path``. Returns the root."""

    def _make(*relatives: str) -> Path:
        for relative in relatives:
            path = tmp_path / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.touch()
        return tmp_path

    return _make


def test_repo_tests_tree_follows_the_layout() -> None:
    assert layout_problems(REPO_ROOT / "tests") == []


def test_reports_directories_without_init(make_tree: Callable[..., Path]) -> None:
    root = make_tree("__init__.py", "good/__init__.py", "good/test_a.py", "bad/nested/test_b.py")
    assert layout_problems(root) == ["add an `__init__.py` to tests/bad/nested"]


def test_files_trees_are_data(make_tree: Callable[..., Path]) -> None:
    root = make_tree("x/files/repo/tools/cmd.py", "x/files/data.json", "x/files/deep/notes.txt")
    assert layout_problems(root) == []


def test_non_python_files_outside_files_are_reported(make_tree: Callable[..., Path]) -> None:
    root = make_tree(
        "__init__.py", "data.json", "pkg/__init__.py", "pkg/files/ok.json", "pkg/fixture.txt"
    )
    assert layout_problems(root) == [
        "move tests/data.json under a `files/` directory next to the tests that use it",
        "move tests/pkg/fixture.txt under a `files/` directory next to the tests that use it",
    ]


def test_ignores_bytecode_caches_and_dotfiles(make_tree: Callable[..., Path]) -> None:
    root = make_tree(
        "__init__.py",
        "__pycache__/x.cpython-311.pyc",
        "__pycache__/y.py",
        ".gitkeep",
        "sub/.DS_Store",
    )
    assert layout_problems(root) == []


def test_top_level_test_modules_are_reported(make_tree: Callable[..., Path]) -> None:
    root = make_tree(
        "__init__.py", "conftest.py", "test_a.py", "area/__init__.py", "area/test_b.py"
    )
    assert layout_problems(root) == [
        "move tests/test_a.py into a `tests/<area>/` directory that mirrors the source it covers"
    ]


def test_top_level_support_modules_are_allowed(make_tree: Callable[..., Path]) -> None:
    root = make_tree("__init__.py", "conftest.py", "_git.py", "area/__init__.py", "area/test_b.py")
    assert layout_problems(root) == []


@pytest.mark.skip_if_binaries_missing("cargo")
def test_command_passes_on_the_repo() -> None:
    """Run the real ``cargo xtask check-test-modules`` against this workspace."""
    check_tests_layout(make_context(REPO_ROOT))


@pytest.mark.skip_if_binaries_missing("cargo")
def test_command_runs_the_xtask_rust_check() -> None:
    run = RecordingRun(("", "", 0))
    check_tests_layout(make_context(REPO_ROOT, run=run))
    assert run.calls == [(shutil.which("cargo"), "xtask", "check-test-modules")]


@pytest.mark.skip_if_binaries_missing("cargo")
def test_inline_rust_test_module_fails_the_command() -> None:
    run = RecordingRun(("", "crates/x/src/lib.rs: inline test module `tests`", 1))
    with pytest.raises(SystemExit) as excinfo:
        check_tests_layout(make_context(REPO_ROOT, run=run))
    assert excinfo.value.code == 1


@pytest.mark.skip_if_binaries_missing("cargo")
def test_python_layout_problem_fails_the_command(make_tree: Callable[..., Path]) -> None:
    root = make_tree("tests/test_top.py")
    run = RecordingRun(("", "", 0))
    ctx = make_context(root, run=run)
    with pytest.raises(SystemExit) as excinfo:
        check_tests_layout(ctx)
    assert excinfo.value.code == 1
    assert "move tests/test_top.py" in ctx.stderr
    assert len(run.calls) == 1
