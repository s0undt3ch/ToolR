"""Tests for ``toolr pre-commit check-test-layout``."""

from __future__ import annotations

from pathlib import Path

from tools.ci import REPO_ROOT
from tools.precommit.tests_layout import layout_problems


def touch(root: Path, relative: str) -> None:
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.touch()


def test_repo_tests_tree_follows_the_layout() -> None:
    assert layout_problems(REPO_ROOT / "tests") == []


def test_reports_directories_without_init(tmp_path: Path) -> None:
    for relative in ("__init__.py", "good/__init__.py", "good/test_a.py", "bad/nested/test_b.py"):
        touch(tmp_path, relative)
    assert layout_problems(tmp_path) == ["tests/bad/nested has no __init__.py"]


def test_coverage_support_exemption_is_exact(tmp_path: Path) -> None:
    touch(tmp_path, "support/coverage/sitecustomize.py")
    touch(tmp_path, "support/other/helper.py")
    assert layout_problems(tmp_path) == ["tests/support/other has no __init__.py"]


def test_files_trees_are_data(tmp_path: Path) -> None:
    for relative in ("x/files/repo/tools/cmd.py", "x/files/data.json", "x/files/deep/notes.txt"):
        touch(tmp_path, relative)
    assert layout_problems(tmp_path) == []


def test_non_python_files_outside_files_are_reported(tmp_path: Path) -> None:
    for relative in (
        "__init__.py",
        "data.json",
        "pkg/__init__.py",
        "pkg/files/ok.json",
        "pkg/fixture.txt",
    ):
        touch(tmp_path, relative)
    assert layout_problems(tmp_path) == [
        "tests/data.json is not a .py file and is outside a files/ directory",
        "tests/pkg/fixture.txt is not a .py file and is outside a files/ directory",
    ]


def test_ignores_bytecode_caches_and_dotfiles(tmp_path: Path) -> None:
    for relative in (
        "__init__.py",
        "__pycache__/x.cpython-311.pyc",
        "__pycache__/y.py",
        ".gitkeep",
        "sub/.DS_Store",
    ):
        touch(tmp_path, relative)
    assert layout_problems(tmp_path) == []
