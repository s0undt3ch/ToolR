"""Check every hand-kept Python version list against ``requires-python``.

``project.requires-python`` in ``crates/toolr-py/pyproject.toml`` is the source of truth for the supported
CPython range. The lists below repeat it, in full or as a floor, and nothing else ties them together.
``MIN_PYTHON_MINOR`` is checked by ``crates/toolr-core/tests/python_floor_lockstep.rs`` instead.
"""

from __future__ import annotations

import re
import tomllib
from functools import partial
from typing import Any

import pytest
from packaging.specifiers import SpecifierSet
from packaging.version import Version
from tools.ci import ABI3_WHEEL_PYTHONS
from tools.ci import ALL_CPYTHONS
from tools.ci import BINARY_WHEEL_PYTHONS
from tools.ci import REPO_ROOT

SOURCE = "requires-python in crates/toolr-py/pyproject.toml"

TOML_FILES = {
    "toolr-py": "crates/toolr-py/pyproject.toml",
    "toolr": "crates/toolr/pyproject.toml",
    "tools": "tools/pyproject.toml",
    "example-plugin": "examples/plugin-package/pyproject.toml",
    "skill-example": "skills/toolr-command-packaging/examples/pyproject.toml",
    "root": "pyproject.toml",
    "mise": "mise.toml",
    "cargo": "Cargo.toml",
}

# Workflow steps that hard-code the abi3 wheel's cp-tag to find the downloaded artefact, either as
# `echo "cp-tag=cp311"` or as a `cptag="cp311"` shell variable.
CP_TAG_WORKFLOWS = [
    ".github/workflows/ci.yml",
    ".github/workflows/_bench.yml",
    ".github/workflows/_test.yml",
]
CP_TAG_PATTERN = re.compile(r"""\b(?:cp-tag=|cptag=)["']?(cp\d+)""")
# A `#` at the start of a line or after whitespace opens a YAML or shell comment.
COMMENT_PATTERN = re.compile(r"(?:^|\s)#.*$", re.MULTILINE)


def python3_floor(requires_python: str) -> int:
    """Return ``N`` from the ``>=3.N`` clause, rejecting a patch-level floor as the Rust lockstep test does."""
    for specifier in SpecifierSet(requires_python):
        release = Version(specifier.version).release
        if specifier.operator == ">=" and release[0] == 3 and len(release) > 1:
            if len(release) > 2 and release[2] != 0:
                msg = (
                    f"requires-python {requires_python!r} sets a patch-level floor, which the "
                    "version lists (and MIN_PYTHON_MINOR) can't express; use `>=3.N`"
                )
                raise ValueError(msg)
            return release[1]
    msg = f"requires-python {requires_python!r} has no `>=3.N` clause"
    raise ValueError(msg)


def supported_minors(requires_python: str) -> list[int]:
    """Return the CPython 3 minors ``requires_python`` admits; it needs both a floor and a `<3.N` ceiling."""
    spec = SpecifierSet(requires_python)
    ceilings = [
        Version(specifier.version).release
        for specifier in spec
        if specifier.operator in {"<", "<="}
    ]
    ceiling = min(
        (release[1] for release in ceilings if release[0] == 3 and len(release) > 1), default=None
    )
    if ceiling is None:
        msg = (
            f"requires-python {requires_python!r} needs an explicit `<3.N` upper bound; "
            "the version lists are derived from it"
        )
        raise ValueError(msg)
    floor = python3_floor(requires_python)
    return [minor for minor in range(floor, ceiling + 1) if f"3.{minor}" in spec]


@pytest.fixture(scope="module")
def pyprojects() -> dict[str, dict[str, Any]]:
    return {
        key: tomllib.loads((REPO_ROOT / path).read_text(encoding="utf-8"))
        for key, path in TOML_FILES.items()
    }


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


def test_python3_floor_rejects_a_patch_floor() -> None:
    with pytest.raises(ValueError, match="patch-level floor"):
        python3_floor(">=3.11.4")


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
    """Return ``(label, location, actual, expected)`` for every setting pinned to the floor.

    A ``requires-python`` floor is a ``partial`` so a malformed one fails only its own case.
    """
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
            "binary-wheel-pythons",
            "tools/ci.py BINARY_WHEEL_PYTHONS",
            BINARY_WHEEL_PYTHONS,
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
            partial(python3_floor, pyprojects["toolr"]["project"]["requires-python"]),
            floor,
        ),
        (
            "tools-requires-python",
            "tools/pyproject.toml project.requires-python floor",
            partial(python3_floor, pyprojects["tools"]["project"]["requires-python"]),
            floor,
        ),
        (
            "example-plugin-requires-python",
            "examples/plugin-package/pyproject.toml project.requires-python floor",
            partial(python3_floor, pyprojects["example-plugin"]["project"]["requires-python"]),
            floor,
        ),
        (
            "skill-example-requires-python",
            "skills/toolr-command-packaging/examples/pyproject.toml project.requires-python floor",
            partial(python3_floor, pyprojects["skill-example"]["project"]["requires-python"]),
            floor,
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
        live = COMMENT_PATTERN.sub("", (REPO_ROOT / workflow).read_text(encoding="utf-8"))
        settings.append(
            (
                f"cp-tag:{workflow}",
                f"{workflow} cp-tag assignments outside comments (expected exactly one)",
                CP_TAG_PATTERN.findall(live),
                [f"cp3{floor}"],
            )
        )
    return settings


FLOOR_LABELS = [
    "pyo3-abi3",
    "abi3-wheel-pythons",
    "binary-wheel-pythons",
    "toolr-py-cibuildwheel",
    "toolr-cibuildwheel",
    "toolr-requires-python",
    "tools-requires-python",
    "example-plugin-requires-python",
    "skill-example-requires-python",
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
    if callable(actual):
        actual = actual()
    assert actual == expected, f"{location} is {actual!r}; {SOURCE} implies {expected!r}"
