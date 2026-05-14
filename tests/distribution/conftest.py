"""Fixtures for distribution-shape tests.

Consumes pre-built wheels from `wheelhouse/` at the repo root. CI's
`distribution-tests` job downloads the cibuildwheel artifacts into
`wheelhouse/` and runs `pytest tests/distribution/`. Locally, drop
wheel files into `wheelhouse/` (e.g. `maturin build --release
-m crates/toolr/Cargo.toml --out wheelhouse`) before running.

If no wheels are present, fixtures skip with a clear reason -- so the
default `pytest tests/` invocation (which doesn't populate
`wheelhouse/`) won't error on these tests.
"""

from __future__ import annotations

from pathlib import Path
from zipfile import ZipFile

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
WHEELHOUSE_DIR = REPO_ROOT / "wheelhouse"


def _binary_wheels() -> list[Path]:
    """All `toolr` (binary) wheels currently in `wheelhouse/`."""
    return sorted(WHEELHOUSE_DIR.glob("toolr-*-py3-none-*.whl"))


def _py_wheels() -> list[Path]:
    """All `toolr_py` (pyo3) wheels currently in `wheelhouse/`."""
    return sorted(WHEELHOUSE_DIR.glob("toolr_py-*-cp*-*.whl"))


def pytest_generate_tests(metafunc: pytest.Metafunc) -> None:
    """Parametrize wheel fixtures over whatever's in `wheelhouse/`."""
    if "toolr_wheel" in metafunc.fixturenames:
        wheels = _binary_wheels()
        if wheels:
            metafunc.parametrize("toolr_wheel", wheels, ids=[w.name for w in wheels])
    if "toolr_py_wheel" in metafunc.fixturenames:
        wheels = _py_wheels()
        if wheels:
            metafunc.parametrize("toolr_py_wheel", wheels, ids=[w.name for w in wheels])


@pytest.fixture(scope="session")
def toolr_wheel() -> Path:
    """Fallback: only reached when `pytest_generate_tests` didn't parametrize.

    Means no matching wheel was found in `wheelhouse/`; skip the test
    with a clear reason rather than erroring.
    """
    pytest.skip(f"no toolr (binary) wheel in {WHEELHOUSE_DIR}/")


@pytest.fixture(scope="session")
def toolr_py_wheel() -> Path:
    """Fallback: same pattern as `toolr_wheel`."""
    pytest.skip(f"no toolr-py (pyo3) wheel in {WHEELHOUSE_DIR}/")


def wheel_namelist(wheel: Path) -> list[str]:
    with ZipFile(wheel) as zf:
        return sorted(zf.namelist())
