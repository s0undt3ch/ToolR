"""Shape assertions for the `toolr` (binary) wheel."""

from __future__ import annotations

from pathlib import Path

from tests.distribution.conftest import wheel_namelist


def test_toolr_wheel_ships_binary(toolr_wheel: Path) -> None:
    names = wheel_namelist(toolr_wheel)
    binary_entries = [n for n in names if "/scripts/toolr" in n]
    assert binary_entries, (
        f"binary wheel must ship `toolr` under <wheel>.data/scripts/, got: {names}"
    )


def test_toolr_wheel_has_no_python_source(toolr_wheel: Path) -> None:
    names = wheel_namelist(toolr_wheel)
    py_files = [n for n in names if n.endswith(".py")]
    assert not py_files, f"binary wheel should not carry Python source, got: {py_files}"


def test_toolr_wheel_has_no_dynlib(toolr_wheel: Path) -> None:
    names = wheel_namelist(toolr_wheel)
    dynlibs = [n for n in names if n.endswith((".so", ".pyd", ".dylib"))]
    assert not dynlibs, f"binary wheel should not carry a pyo3 dynlib, got: {dynlibs}"


def test_toolr_wheel_filename_is_universal_python(toolr_wheel: Path) -> None:
    """Binary wheels don't link Python; tag should be py3-none-*."""
    assert "py3-none-" in toolr_wheel.name, (
        f"binary wheel filename should carry py3-none- tag, got: {toolr_wheel.name}"
    )
