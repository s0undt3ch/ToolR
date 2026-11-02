"""Check every hand-kept Python version list against ``requires-python``.

``project.requires-python`` in ``crates/toolr-py/pyproject.toml`` is the source of truth for the supported
CPython range. The lists below repeat it, in full or as a floor, and nothing else ties them together.
``MIN_PYTHON_MINOR`` is checked by ``crates/toolr-core/tests/python_floor_lockstep.rs`` instead.
"""

from __future__ import annotations

import re
import tomllib
from typing import Any

import pytest
from packaging.specifiers import SpecifierSet
from tools.ci import ABI3_WHEEL_PYTHONS
from tools.ci import ALL_CPYTHONS
from tools.ci import REPO_ROOT

SOURCE = "requires-python in crates/toolr-py/pyproject.toml"

TOML_FILES = {
    "toolr-py": "crates/toolr-py/pyproject.toml",
    "toolr": "crates/toolr/pyproject.toml",
    "root": "pyproject.toml",
    "mise": "mise.toml",
    "cargo": "Cargo.toml",
}

# Workflow steps that hard-code the abi3 wheel's cp-tag to find the downloaded artefact.
CP_TAG_WORKFLOWS = [".github/workflows/ci.yml", ".github/workflows/_bench.yml"]


def supported_minors(requires_python: str) -> list[int]:
    """Return the CPython 3 minors ``requires_python`` admits, capped at 3.39 when unbounded."""
    spec = SpecifierSet(requires_python)
    return [minor for minor in range(40) if f"3.{minor}" in spec]


@pytest.fixture(scope="module")
def pyprojects() -> dict[str, dict[str, Any]]:
    return {key: tomllib.loads((REPO_ROOT / path).read_text()) for key, path in TOML_FILES.items()}


@pytest.fixture(scope="module")
def supported(pyprojects: dict[str, dict[str, Any]]) -> list[int]:
    minors = supported_minors(pyprojects["toolr-py"]["project"]["requires-python"])
    assert minors, f"{SOURCE} admits no CPython 3 minor"
    return minors


@pytest.mark.parametrize(
    ("requires_python", "expected"),
    [
        (">=3.11,<3.16", list(range(11, 16))),
        (">=3.11,<=3.15", list(range(11, 16))),
        (">=3.12", list(range(12, 40))),
    ],
)
def test_supported_minors_reads_bounds(requires_python: str, expected: list[int]) -> None:
    assert supported_minors(requires_python) == expected


def test_all_cpythons_match(supported: list[int]) -> None:
    assert ALL_CPYTHONS, "tools/ci.py ALL_CPYTHONS is empty"
    assert sorted(ALL_CPYTHONS) == ALL_CPYTHONS, (
        f"tools/ci.py ALL_CPYTHONS is {ALL_CPYTHONS}; it must be sorted ascending (TEST_PYTHONS_NARROW takes the last)"
    )
    actual = [int(tag.removeprefix("cp3")) for tag in ALL_CPYTHONS]
    assert actual == supported, (
        f"tools/ci.py ALL_CPYTHONS is {ALL_CPYTHONS}; {SOURCE} implies minors {supported}"
    )


def test_classifiers_match(pyprojects: dict[str, dict[str, Any]], supported: list[int]) -> None:
    prefix = "Programming Language :: Python :: 3."
    actual = sorted(
        int(classifier.removeprefix(prefix))
        for classifier in pyprojects["toolr-py"]["project"]["classifiers"]
        if classifier.startswith(prefix) and classifier.removeprefix(prefix).isdigit()
    )
    assert actual, "crates/toolr-py/pyproject.toml project.classifiers has no Python 3.N classifier"
    assert actual == supported, (
        f"crates/toolr-py/pyproject.toml project.classifiers lists minors {actual}; {SOURCE} implies {supported}"
    )


def test_mise_pythons_match(pyprojects: dict[str, dict[str, Any]], supported: list[int]) -> None:
    versions = pyprojects["mise"]["tools"]["python"]
    assert versions, "mise.toml [tools] python is empty"
    # Only the second dot-separated field matters, so "3.15.2" and "3.16.0rc1" both parse.
    actual = sorted({int(version.split(".")[1]) for version in versions})
    assert actual == supported, (
        f"mise.toml [tools] python is {versions}; {SOURCE} implies minors {supported}"
    )


def _floor_settings(
    pyprojects: dict[str, dict[str, Any]], floor: int
) -> list[tuple[str, str, object, object]]:
    """Return ``(label, location, actual, expected)`` for every setting pinned to the floor."""
    pyo3_features = pyprojects["cargo"]["workspace"]["dependencies"]["pyo3"]["features"]
    abi3 = [feature for feature in pyo3_features if feature.startswith("abi3-")]
    settings: list[tuple[str, str, object, object]] = [
        (
            "pyo3-abi3",
            "Cargo.toml workspace.dependencies.pyo3.features abi3-*",
            abi3,
            [f"abi3-py3{floor}"],
        ),
        (
            "abi3-wheel-pythons",
            "tools/ci.py ABI3_WHEEL_PYTHONS",
            ABI3_WHEEL_PYTHONS,
            [f"cp3{floor}"],
        ),
        (
            "toolr-py-cibuildwheel",
            "crates/toolr-py/pyproject.toml tool.cibuildwheel.build",
            pyprojects["toolr-py"]["tool"]["cibuildwheel"]["build"],
            f"cp3{floor}-*",
        ),
        (
            "toolr-cibuildwheel",
            "crates/toolr/pyproject.toml tool.cibuildwheel.build",
            pyprojects["toolr"]["tool"]["cibuildwheel"]["build"],
            f"cp3{floor}-*",
        ),
        (
            "toolr-requires-python",
            "crates/toolr/pyproject.toml project.requires-python floor",
            supported_minors(pyprojects["toolr"]["project"]["requires-python"])[:1],
            [floor],
        ),
        (
            "ruff",
            "pyproject.toml tool.ruff.target-version",
            pyprojects["root"]["tool"]["ruff"]["target-version"],
            f"py3{floor}",
        ),
        (
            "mypy",
            "pyproject.toml tool.mypy.python_version",
            pyprojects["root"]["tool"]["mypy"]["python_version"],
            f"3.{floor}",
        ),
    ]
    for workflow in CP_TAG_WORKFLOWS:
        tags = re.findall(r"cp-tag=(cp\d+)", (REPO_ROOT / workflow).read_text())
        settings.append(
            (
                f"cp-tag:{workflow}",
                f"{workflow} cp-tag output",
                tags or None,
                [f"cp3{floor}"] * max(len(tags), 1),
            )
        )
    return settings


FLOOR_LABELS = [
    "pyo3-abi3",
    "abi3-wheel-pythons",
    "toolr-py-cibuildwheel",
    "toolr-cibuildwheel",
    "toolr-requires-python",
    "ruff",
    "mypy",
    *(f"cp-tag:{workflow}" for workflow in CP_TAG_WORKFLOWS),
]


@pytest.mark.parametrize("label", FLOOR_LABELS)
def test_floor_settings_match(
    label: str, pyprojects: dict[str, dict[str, Any]], supported: list[int]
) -> None:
    floor = min(supported)
    settings = {entry[0]: entry[1:] for entry in _floor_settings(pyprojects, floor)}
    assert settings.keys() == set(FLOOR_LABELS)
    location, actual, expected = settings[label]
    assert actual == expected, f"{location} is {actual!r}; {SOURCE} implies {expected!r}"
