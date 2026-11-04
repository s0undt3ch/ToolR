"""Tests for ``toolr pre-commit check-python-versions`` and the lists it checks against ``requires-python``."""

from __future__ import annotations

import re
import shutil
from collections.abc import Callable
from pathlib import Path

import pytest
import yaml

import tools.precommit.python_versions as python_versions_module
from tools.ci import REPO_ROOT
from tools.precommit.python_versions import CONFIG_TOML_FILES
from tools.precommit.python_versions import CP_TAG_WORKFLOWS
from tools.precommit.python_versions import python_version_mismatches


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


def test_hook_files_cover_inputs() -> None:
    config = yaml.safe_load((REPO_ROOT / ".pre-commit-config.yaml").read_text(encoding="utf-8"))
    hooks = [
        hook
        for repo in config["repos"]
        if repo["repo"] == "local"
        for hook in repo["hooks"]
        if hook["id"] == "check-python-versions"
    ]
    assert len(hooks) == 1, hooks
    files = re.compile(hooks[0]["files"])
    inputs = [
        *CONFIG_TOML_FILES.values(),
        *CP_TAG_WORKFLOWS,
        "tools/ci.py",
        "tools/precommit/python_versions.py",
    ]
    assert [path for path in inputs if not files.search(path)] == []
    assert not files.search("README.md")


@pytest.mark.parametrize(
    ("name", "location"),
    [
        ("ABI3_WHEEL_PYTHONS", "tools/ci.py ABI3_WHEEL_PYTHONS"),
        ("BINARY_WHEEL_PYTHONS", "tools/ci.py BINARY_WHEEL_PYTHONS"),
    ],
)
def test_tools_ci_wheel_list_drift_reported(
    make_python_versions_tree: Callable[[], Path],
    monkeypatch: pytest.MonkeyPatch,
    name: str,
    location: str,
) -> None:
    monkeypatch.setattr(python_versions_module, name, ["cp399"])

    mismatches = python_version_mismatches(make_python_versions_tree())

    assert len(mismatches) == 1, mismatches
    assert location in mismatches[0]
