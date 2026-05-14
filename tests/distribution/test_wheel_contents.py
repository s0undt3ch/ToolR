"""Tests covering the contents of the `toolr` wheel."""

from __future__ import annotations

import shutil
import subprocess
import sys
import zipfile
from collections.abc import Callable
from pathlib import Path

import pytest

pytestmark = pytest.mark.skipif(
    shutil.which("maturin") is None,
    reason="maturin not on PATH",
)

REPO_ROOT = Path(__file__).resolve().parents[2]


@pytest.fixture
def built_wheel(tmp_path: Path) -> Callable[[], Path]:
    """Factory: build a wheel into ``tmp_path/wheelhouse`` and return its path."""

    def _build() -> Path:
        out_dir = tmp_path / "wheelhouse"
        out_dir.mkdir()
        maturin = shutil.which("maturin")
        assert maturin is not None, "maturin missing despite pytestmark guard"
        subprocess.run(  # noqa: S603
            [
                maturin,
                "build",
                "--release",
                "--out",
                str(out_dir),
                "--interpreter",
                sys.executable,
            ],
            cwd=REPO_ROOT,
            check=True,
        )
        wheels = list(out_dir.glob("toolr-*.whl"))
        assert len(wheels) == 1, f"expected one wheel, got {wheels}"
        return wheels[0]

    return _build


def _expected_bin_name() -> str:
    return "toolr.exe" if sys.platform == "win32" else "toolr"


@pytest.mark.xfail(
    reason=(
        "maturin in pyo3-bindings mode does not include [[bin]] targets in "
        "wheels. The standalone toolr binary ships via install.sh, mise, and "
        "GitHub release archives instead. Tracking issue: revisit if maturin "
        "ships bin-in-pyo3-wheel support."
    ),
    strict=False,
)
def test_wheel_includes_rust_binary(built_wheel: Callable[[], Path]) -> None:
    wheel = built_wheel()
    with zipfile.ZipFile(wheel) as zf:
        names = zf.namelist()
    binary_name = _expected_bin_name()
    candidates = [n for n in names if n.endswith(f"data/scripts/{binary_name}")]
    assert candidates, f"expected `data/scripts/{binary_name}` inside wheel, got names: {names[:20]}..."


@pytest.mark.xfail(
    reason=(
        "Workspace split (Plan 12 Stage 5+6): the root pyproject.toml no longer "
        "exposes a [build-system], so `maturin build` from REPO_ROOT fails. The "
        "Python package now ships in the separate `toolr-py` wheel, built from "
        "`crates/toolr-py/pyproject.toml`. Stage 11 replaces this whole test "
        "file with per-wheel shape assertions in test_toolr_wheel.py / "
        "test_toolr_py_wheel.py."
    ),
    strict=False,
)
def test_wheel_includes_python_package(built_wheel: Callable[[], Path]) -> None:
    wheel = built_wheel()
    with zipfile.ZipFile(wheel) as zf:
        names = zf.namelist()
    assert any(n.endswith("toolr/__init__.py") for n in names), (
        f"expected `toolr/__init__.py` inside wheel, got names: {names[:20]}..."
    )
    assert any(n.endswith("toolr/_runner.py") for n in names) or any(n.endswith("toolr/__main__.py") for n in names), (
        "expected python package modules in wheel"
    )
