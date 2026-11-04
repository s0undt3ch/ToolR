"""Check every hand-kept Python version list against ``requires-python``.

``project.requires-python`` in ``crates/toolr-py/pyproject.toml`` is the source of truth for the supported
CPython range. The lists below repeat it, in full or as a floor, and nothing else ties them together.
``MIN_PYTHON_MINOR`` is checked by ``crates/toolr-core/tests/python_floor_lockstep.rs`` instead.
"""

from __future__ import annotations

import re
import tomllib
from pathlib import Path
from typing import Any
from typing import NamedTuple

import pytest
from tools.ci import ABI3_WHEEL_PYTHONS
from tools.ci import BINARY_WHEEL_PYTHONS
from tools.ci import REPO_ROOT
from tools.ci import _requires_python
from tools.ci import python3_floor
from tools.ci import supported_minors

REQUIRES_PYTHON_SOURCE = "requires-python in crates/toolr-py/pyproject.toml"

CONFIG_TOML_FILES = {
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


class FloorSetting(NamedTuple):
    label: str
    location: str
    actual: object
    expected: object


@pytest.fixture(scope="module")
def config_tomls() -> dict[str, dict[str, Any]]:
    return {
        key: tomllib.loads((REPO_ROOT / path).read_text(encoding="utf-8"))
        for key, path in CONFIG_TOML_FILES.items()
    }


@pytest.fixture(scope="module")
def supported(config_tomls: dict[str, dict[str, Any]]) -> list[int]:
    minors = supported_minors(config_tomls["toolr-py"]["project"]["requires-python"])
    assert minors, f"{REQUIRES_PYTHON_SOURCE} admits no CPython 3 minor"
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


def test_requires_python_reads_project_table(tmp_path: Path) -> None:
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text(
        '[project]\nname = "x"\nrequires-python = ">=3.12,<3.14"\n\n[tool.x]\nrequires-python = "nope"\n',
        encoding="utf-8",
    )
    assert _requires_python(pyproject) == ">=3.12,<3.14"


def test_classifiers_match(config_tomls: dict[str, dict[str, Any]], supported: list[int]) -> None:
    prefix = "Programming Language :: Python :: 3."
    actual = sorted(
        int(classifier.removeprefix(prefix))
        for classifier in config_tomls["toolr-py"]["project"]["classifiers"]
        if classifier.startswith(prefix) and classifier.removeprefix(prefix).isdigit()
    )
    assert actual == supported, (
        f"crates/toolr-py/pyproject.toml project.classifiers lists minors {actual}; "
        f"{REQUIRES_PYTHON_SOURCE} implies {supported}"
    )


def test_mise_pythons_match(config_tomls: dict[str, dict[str, Any]], supported: list[int]) -> None:
    versions = config_tomls["mise"]["tools"]["python"]
    # Only the second dot-separated field matters, so "3.15.2" and "3.16.0rc1" both parse.
    actual = sorted({int(version.split(".")[1]) for version in versions})
    assert actual == supported, (
        f"mise.toml [tools] python is {versions}; {REQUIRES_PYTHON_SOURCE} implies minors {supported}"
    )


def _floor_settings(config_tomls: dict[str, dict[str, Any]], floor: int) -> list[FloorSetting]:
    pyo3_features = config_tomls["cargo"]["workspace"]["dependencies"]["pyo3"]["features"]
    settings = [
        FloorSetting(
            "pyo3-abi3",
            "Cargo.toml workspace.dependencies.pyo3.features abi3-*",
            [feature for feature in pyo3_features if feature.startswith("abi3-")],
            [f"abi3-py3{floor}"],
        ),
        FloorSetting(
            "abi3-wheel-pythons",
            "tools/ci.py ABI3_WHEEL_PYTHONS",
            ABI3_WHEEL_PYTHONS,
            [f"cp3{floor}"],
        ),
        FloorSetting(
            "binary-wheel-pythons",
            "tools/ci.py BINARY_WHEEL_PYTHONS",
            BINARY_WHEEL_PYTHONS,
            [f"cp3{floor}"],
        ),
        FloorSetting(
            "toolr-py-cibuildwheel",
            "crates/toolr-py/pyproject.toml tool.cibuildwheel.build",
            config_tomls["toolr-py"]["tool"]["cibuildwheel"]["build"],
            f"cp3{floor}-*",
        ),
        FloorSetting(
            "toolr-cibuildwheel",
            "crates/toolr/pyproject.toml tool.cibuildwheel.build",
            config_tomls["toolr"]["tool"]["cibuildwheel"]["build"],
            f"cp3{floor}-*",
        ),
        FloorSetting(
            "ruff",
            "pyproject.toml tool.ruff.target-version",
            config_tomls["root"]["tool"]["ruff"]["target-version"],
            f"py3{floor}",
        ),
        FloorSetting(
            "mypy",
            "pyproject.toml tool.mypy.python_version",
            config_tomls["root"]["tool"]["mypy"]["python_version"],
            f"3.{floor}",
        ),
    ]
    for workflow in CP_TAG_WORKFLOWS:
        live = COMMENT_PATTERN.sub("", (REPO_ROOT / workflow).read_text(encoding="utf-8"))
        settings.append(
            FloorSetting(
                f"cp-tag:{workflow}",
                f"{workflow} cp-tag assignments outside comments (expected exactly one)",
                CP_TAG_PATTERN.findall(live),
                [f"cp3{floor}"],
            )
        )
    return settings


def test_floor_settings_match(
    config_tomls: dict[str, dict[str, Any]], supported: list[int], subtests: pytest.Subtests
) -> None:
    for setting in _floor_settings(config_tomls, min(supported)):
        with subtests.test(setting.label):
            assert setting.actual == setting.expected, (
                f"{setting.location} is {setting.actual!r}; {REQUIRES_PYTHON_SOURCE} implies {setting.expected!r}"
            )


# Parsed inside each case, so a malformed floor fails only its own file.
@pytest.mark.parametrize("key", ["toolr", "tools", "example-plugin", "skill-example"])
def test_requires_python_floors_match(
    key: str, config_tomls: dict[str, dict[str, Any]], supported: list[int]
) -> None:
    actual = python3_floor(config_tomls[key]["project"]["requires-python"])
    assert actual == min(supported), (
        f"{CONFIG_TOML_FILES[key]} project.requires-python floor is 3.{actual}; "
        f"{REQUIRES_PYTHON_SOURCE} implies 3.{min(supported)}"
    )
