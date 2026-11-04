"""Tests for the ``requires-python`` derivation and the Python version-list checks.

``project.requires-python`` in ``crates/toolr-py/pyproject.toml`` is the source of truth for the supported
CPython range. ``tools/ci.py`` derives ``ALL_CPYTHONS`` from it, and ``toolr pre-commit check-python-versions``
checks the hand-kept lists that repeat it. ``MIN_PYTHON_MINOR`` is checked by
``crates/toolr-core/tests/python_floor_lockstep.rs`` instead.
"""

from __future__ import annotations

import re
import shutil
from collections.abc import Callable
from pathlib import Path

import pytest
from tools.ci import REPO_ROOT
from tools.ci import _requires_python
from tools.ci import python3_floor
from tools.ci import supported_minors
from tools.precommit.python_versions import CONFIG_TOML_FILES
from tools.precommit.python_versions import CP_TAG_WORKFLOWS
from tools.precommit.python_versions import python_version_mismatches


@pytest.mark.parametrize(
    ("requires_python", "expected"),
    [
        (">=3.11,<3.16", list(range(11, 16))),
        (">=3.11,<=3.15", list(range(11, 16))),
        ("<3.16, >= 3.12", list(range(12, 16))),
        (">=3.12.0rc1,<3.14", [12, 13]),
    ],
)
def test_supported_minors_reads_bounds(requires_python: str, expected: list[int]) -> None:
    assert supported_minors(requires_python) == expected


@pytest.mark.parametrize(
    ("requires_python", "error"),
    [
        (">=3.11.4,<3.16", "patch-level floor"),
        (">=3.11", "explicit `<3.N` upper bound"),
        ("~=3.11", "explicit `<3.N` upper bound"),
        (">=3.11,<4", "explicit `<3.N` upper bound"),
        ("<3.16", "no `>=3.N` clause"),
    ],
)
def test_supported_minors_rejects(requires_python: str, error: str) -> None:
    with pytest.raises(ValueError, match=re.escape(error)):
        supported_minors(requires_python)


@pytest.mark.parametrize(
    ("requires_python", "expected"),
    [(">=3.11", 11), (">= 3.12, <3.16", 12), (">=3.12.0rc1", 12)],
)
def test_python3_floor_reads_the_lower_bound(requires_python: str, expected: int) -> None:
    assert python3_floor(requires_python) == expected


def test_requires_python_reads_project_table(tmp_path: Path) -> None:
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text(
        '[project]\nname = "x"\nrequires-python = ">=3.12,<3.14"\n\n[tool.x]\nrequires-python = "nope"\n',
        encoding="utf-8",
    )
    assert _requires_python(pyproject) == ">=3.12,<3.14"


def test_repo_python_versions_agree() -> None:
    assert python_version_mismatches(REPO_ROOT) == []


@pytest.fixture
def make_python_versions_tree(tmp_path: Path) -> Callable[[], Path]:
    """Copy every checker input into ``tmp_path``, keeping repo-relative paths."""

    def factory() -> Path:
        for relative in [*CONFIG_TOML_FILES.values(), *CP_TAG_WORKFLOWS]:
            destination = tmp_path / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REPO_ROOT / relative, destination)
        return tmp_path

    return factory


@pytest.mark.parametrize(
    ("path", "old", "new", "expected_fragment"),
    [
        ("mise.toml", '"3.11.16"', "", "mise.toml [tools] python"),
        (
            "crates/toolr-py/pyproject.toml",
            '"Programming Language :: Python :: 3.15",',
            "",
            "crates/toolr-py/pyproject.toml project.classifiers",
        ),
        (
            "Cargo.toml",
            '"abi3-py311"',
            '"abi3-py312"',
            "Cargo.toml workspace.dependencies.pyo3.features",
        ),
        (
            ".github/workflows/_test.yml",
            'cptag="cp311"',
            'cptag="cp312"',
            ".github/workflows/_test.yml cp-tag",
        ),
    ],
    ids=["mise", "classifier", "pyo3-abi3", "workflow-cp-tag"],
)
def test_mismatch_reported(
    make_python_versions_tree: Callable[[], Path],
    path: str,
    old: str,
    new: str,
    expected_fragment: str,
) -> None:
    root = make_python_versions_tree()
    target = root / path
    text = target.read_text(encoding="utf-8")
    assert old in text
    target.write_text(text.replace(old, new, 1), encoding="utf-8")

    mismatches = python_version_mismatches(root)

    assert len(mismatches) == 1, mismatches
    assert expected_fragment in mismatches[0]
