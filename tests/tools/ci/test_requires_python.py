"""Tests for ``supported_minors`` and ``python3_floor``, which derive ``ALL_CPYTHONS`` in ``tools/ci.py``.

``project.requires-python`` in ``crates/toolr-py/pyproject.toml`` is the source of truth for the supported
CPython range. ``MIN_PYTHON_MINOR`` is checked by ``crates/toolr-core/tests/python_floor_lockstep.rs`` instead.
"""

from __future__ import annotations

import re

import pytest

from tools.ci import python3_floor
from tools.ci import supported_minors


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
