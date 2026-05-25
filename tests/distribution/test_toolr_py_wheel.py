"""Shape assertions for the `toolr-py` (pyo3) wheel."""

from __future__ import annotations

from pathlib import Path

from tests.distribution.conftest import wheel_namelist

EXPECTED_PRESENT = [
    "toolr/__init__.py",
    "toolr/_context.py",
    "toolr/_context.pyi",
    "toolr/_exc.py",
    "toolr/py.typed",
    "toolr/testing.py",
    "toolr/types/__init__.py",
    "toolr/utils/__init__.py",
    "toolr/utils/_console.py",
    "toolr/utils/_docstrings.py",
    "toolr/utils/_imports.py",
    "toolr/utils/_logs.py",
    "toolr/utils/_signature.py",
    "toolr/utils/_rust_utils.pyi",
    "toolr/utils/command.py",
]


def test_toolr_py_wheel_contains_python_source(toolr_py_wheel: Path) -> None:
    names = set(wheel_namelist(toolr_py_wheel))
    missing = [p for p in EXPECTED_PRESENT if p not in names]
    assert not missing, f"toolr-py wheel missing expected files: {missing}"


def test_toolr_py_wheel_ships_dynlib(toolr_py_wheel: Path) -> None:
    names = wheel_namelist(toolr_py_wheel)
    dynlibs = [n for n in names if n.startswith("toolr/utils/_rust_utils.") and n.endswith((".so", ".pyd", ".dylib"))]
    assert dynlibs, f"toolr-py wheel missing _rust_utils dynlib, got: {names}"
