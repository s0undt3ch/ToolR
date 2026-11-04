"""
Pre-commit hook command that keeps the hand-kept Python version lists in step with ``requires-python``.
"""

from __future__ import annotations

import re
import tomllib
from pathlib import Path
from typing import Any
from typing import NamedTuple

from rich.markup import escape

from toolr import Context
from tools.ci import ABI3_WHEEL_PYTHONS
from tools.ci import BINARY_WHEEL_PYTHONS
from tools.ci import REPO_ROOT
from tools.ci import python3_floor
from tools.ci import supported_minors

from ._common import group

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
    """A floor-only setting, with the value it holds and the value `requires-python` implies."""

    location: str
    actual: object
    expected: object


def _floor_settings(
    repo_root: Path, config_tomls: dict[str, dict[str, Any]], floor: int
) -> list[FloorSetting]:
    pyo3_features = config_tomls["cargo"]["workspace"]["dependencies"]["pyo3"]["features"]
    settings = [
        FloorSetting(
            "Cargo.toml workspace.dependencies.pyo3.features abi3-*",
            [feature for feature in pyo3_features if feature.startswith("abi3-")],
            [f"abi3-py3{floor}"],
        ),
        FloorSetting("tools/ci.py ABI3_WHEEL_PYTHONS", ABI3_WHEEL_PYTHONS, [f"cp3{floor}"]),
        FloorSetting("tools/ci.py BINARY_WHEEL_PYTHONS", BINARY_WHEEL_PYTHONS, [f"cp3{floor}"]),
        FloorSetting(
            "crates/toolr-py/pyproject.toml tool.cibuildwheel.build",
            config_tomls["toolr-py"]["tool"]["cibuildwheel"]["build"],
            f"cp3{floor}-*",
        ),
        FloorSetting(
            "crates/toolr/pyproject.toml tool.cibuildwheel.build",
            config_tomls["toolr"]["tool"]["cibuildwheel"]["build"],
            f"cp3{floor}-*",
        ),
        FloorSetting(
            "pyproject.toml tool.ruff.target-version",
            config_tomls["root"]["tool"]["ruff"]["target-version"],
            f"py3{floor}",
        ),
        FloorSetting(
            "pyproject.toml tool.mypy.python_version",
            config_tomls["root"]["tool"]["mypy"]["python_version"],
            f"3.{floor}",
        ),
    ]
    for workflow in CP_TAG_WORKFLOWS:
        live = COMMENT_PATTERN.sub("", (repo_root / workflow).read_text(encoding="utf-8"))
        settings.append(
            FloorSetting(
                f"{workflow} cp-tag assignments outside comments (expected exactly one)",
                CP_TAG_PATTERN.findall(live),
                [f"cp3{floor}"],
            )
        )
    return settings


def python_version_mismatches(repo_root: Path) -> list[str]:
    """Return one message per version list under ``repo_root`` that disagrees with its ``requires-python``.

    TOML and workflow inputs are read from ``repo_root``; ``ABI3_WHEEL_PYTHONS`` and
    ``BINARY_WHEEL_PYTHONS`` come from the imported ``tools.ci``, whatever ``repo_root`` is.
    """
    config_tomls = {
        key: tomllib.loads((repo_root / path).read_text(encoding="utf-8"))
        for key, path in CONFIG_TOML_FILES.items()
    }
    supported = supported_minors(config_tomls["toolr-py"]["project"]["requires-python"])
    if not supported:
        return [f"{REQUIRES_PYTHON_SOURCE} admits no CPython 3 minor"]
    floor = min(supported)
    mismatches = []

    prefix = "Programming Language :: Python :: 3."
    classifiers = sorted(
        int(classifier.removeprefix(prefix))
        for classifier in config_tomls["toolr-py"]["project"]["classifiers"]
        if classifier.startswith(prefix) and classifier.removeprefix(prefix).isdigit()
    )
    if classifiers != supported:
        mismatches.append(
            f"crates/toolr-py/pyproject.toml project.classifiers lists minors {classifiers}; "
            f"{REQUIRES_PYTHON_SOURCE} implies {supported}"
        )

    mise_versions = config_tomls["mise"]["tools"]["python"]
    # The minor is the second field of "3.15.2" and "3.16.0rc1" alike.
    mise_minors = sorted({int(version.split(".")[1]) for version in mise_versions})
    if mise_minors != supported:
        mismatches.append(
            f"mise.toml [tools] python is {mise_versions}; "
            f"{REQUIRES_PYTHON_SOURCE} implies minors {supported}"
        )

    for setting in _floor_settings(repo_root, config_tomls, floor):
        if setting.actual != setting.expected:
            mismatches.append(
                f"{setting.location} is {setting.actual!r}; "
                f"{REQUIRES_PYTHON_SOURCE} implies {setting.expected!r}"
            )

    for key in ("toolr", "tools", "example-plugin", "skill-example"):
        location = f"{CONFIG_TOML_FILES[key]} project.requires-python"
        # Report a malformed floor against its own file rather than aborting the whole check.
        try:
            actual = python3_floor(config_tomls[key]["project"]["requires-python"])
        except ValueError as exc:
            mismatches.append(f"{location}: {exc}")
            continue
        if actual != floor:
            mismatches.append(
                f"{location} floor is 3.{actual}; {REQUIRES_PYTHON_SOURCE} implies 3.{floor}"
            )

    return mismatches


@group.command
def check_python_versions(ctx: Context) -> None:
    """Check the hand-kept Python version lists against ``requires-python``.

    ``project.requires-python`` in ``crates/toolr-py/pyproject.toml`` is the source of truth for
    the supported CPython range. The ``mise.toml`` interpreters, the toolr-py classifiers and every
    floor setting (pyo3 ``abi3-*``, cibuildwheel, ruff, mypy, the other ``requires-python`` floors
    and the workflows' hard-coded cp-tags) repeat it by hand; this reports each one that disagrees.
    """
    mismatches = python_version_mismatches(REPO_ROOT)
    for mismatch in mismatches:
        # Messages quote TOML keys like `[tools]`, which rich would eat as markup.
        ctx.error(escape(mismatch))
    if mismatches:
        ctx.error(f"Update the values listed above to match {REQUIRES_PYTHON_SOURCE}.")
        ctx.exit(1)
