"""Fixtures for distribution-shape tests.

Consumes pre-built wheels from `wheelhouse/` at the repo root. CI's
`distribution-tests` job downloads the cibuildwheel artifacts into
`wheelhouse/` and runs `pytest tests/distribution/`. Locally, drop
wheel files into `wheelhouse/` (e.g. `maturin build --release
-m crates/toolr/Cargo.toml --out wheelhouse`) before running.

If no wheels are present, fixtures skip with a clear reason -- so the
default `pytest tests/` invocation (which doesn't populate
`wheelhouse/`) won't error on these tests.

The wheel-fixture parametrization filters to only those wheels that
pip would actually install on the current runner. CI downloads the
full OS-family set (manylinux + musllinux + every arch + every CPython
ABI), so without filtering the matrix would generate dozens of combos
that just exist to fail pip's platform-tag check. Filtering at
parametrization keeps the test surface focused on combinations that
exercise real install paths.
"""

from __future__ import annotations

import dataclasses
import os
import shutil
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from zipfile import ZipFile

import pytest
from packaging.tags import Tag
from packaging.tags import parse_tag
from packaging.tags import sys_tags

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
WHEELHOUSE_DIR = REPO_ROOT / "wheelhouse"
EXAMPLE_PLUGIN_DIR = REPO_ROOT / "examples" / "plugin-package"


def _supported_tags() -> frozenset[Tag]:
    """Cache the current interpreter's installable wheel tags."""
    return frozenset(sys_tags())


def _wheel_tags(wheel: Path) -> frozenset[Tag]:
    """Parse the `{python}-{abi}-{platform}` segments of a wheel filename.

    PEP 427 wheel filenames are `<name>-<version>(-<build>)?-<py>-<abi>-<plat>.whl`.
    `packaging.tags.parse_tag` expands compressed tag triples
    (e.g. `cp311.cp312-abi3-manylinux_2_28_x86_64`) into a frozenset of
    concrete `Tag` instances.
    """
    stem = wheel.stem  # strip `.whl`
    # The last three `-`-separated segments are <py>-<abi>-<plat>.
    py, abi, plat = stem.rsplit("-", 3)[-3:]
    return parse_tag(f"{py}-{abi}-{plat}")


def _installable(wheel: Path) -> bool:
    """True if pip would accept this wheel on the current interpreter."""
    return bool(_wheel_tags(wheel) & _supported_tags())


def _binary_wheels() -> list[Path]:
    """`toolr` (binary) wheels in `wheelhouse/` installable on this runner."""
    return sorted(w for w in WHEELHOUSE_DIR.glob("toolr-*-py3-none-*.whl") if _installable(w))


def _py_wheels() -> list[Path]:
    """`toolr_py` (pyo3) wheels in `wheelhouse/` installable on this runner."""
    return sorted(w for w in WHEELHOUSE_DIR.glob("toolr_py-*-cp*-*.whl") if _installable(w))


def pytest_generate_tests(metafunc: pytest.Metafunc) -> None:
    """Parametrize wheel fixtures over installable wheels in `wheelhouse/`."""
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


@pytest.fixture(scope="session")
def example_plugin_wheel(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """Path to a built wheel of `toolr-plugin-example`.

    `toolr-plugin-example` is pure Python and produces a universal
    `py3-none-any` wheel that's identical across Python versions, archs
    and OSes. To save CI time, the workflow builds it once and drops it
    into `wheelhouse/`; this fixture prefers that prebuilt wheel.

    Falls back to building inline via `uv build` so `pytest
    tests/distribution/` works locally without a separate build step.
    Session-scoped so the (single) inline build is shared across every
    test that uses the fixture.
    """
    wheels = sorted(WHEELHOUSE_DIR.glob("toolr_plugin_example-*.whl"))
    if wheels:
        return wheels[0]

    uv = shutil.which("uv")
    if uv is None:
        pytest.skip("uv required to build the example-plugin wheel")

    out_dir = tmp_path_factory.mktemp("example-plugin-wheel")
    subprocess.run(  # noqa: S603
        [uv, "build", "--wheel", "--out-dir", str(out_dir), str(EXAMPLE_PLUGIN_DIR)],
        check=True,
    )
    built = sorted(out_dir.glob("toolr_plugin_example-*.whl"))
    if len(built) != 1:
        pytest.fail(
            f"`uv build` produced unexpected wheel set under {out_dir}: {built}",
        )
    return built[0]


@dataclasses.dataclass(frozen=True)
class VenvPaths:
    """Paths inside a uv-created venv, with OS-correct layout already resolved."""

    root: Path
    """The venv directory itself (whatever was passed in)."""
    binroot: Path
    """`<venv>/Scripts` on Windows, `<venv>/bin` on Unix."""
    python: Path
    """Path to the Python interpreter."""
    toolr: Path
    """Path to the `toolr` CLI binary once installed (does not have to exist yet)."""


@pytest.fixture
def make_uv_venv() -> Callable[[Path], VenvPaths]:
    """Create a `uv venv --seed` at the given directory and return its paths.

    Branches on `os.name` so Windows (`Scripts/python.exe`) and Unix
    (`bin/python`) layouts both resolve uniformly. The location is
    caller-supplied because some tests need the venv at a specific path
    inside a fake project (e.g. `<project>/tools/.venv/` for toolr's
    in-tree venv discovery).
    """
    uv = shutil.which("uv")
    if uv is None:
        pytest.skip("uv required to create test venvs")

    def _make(venv_dir: Path) -> VenvPaths:
        subprocess.run(  # noqa: S603
            [uv, "venv", "--python", sys.executable, "--seed", str(venv_dir)],
            check=True,
        )
        is_windows = os.name == "nt"
        binroot = venv_dir / ("Scripts" if is_windows else "bin")
        suffix = ".exe" if is_windows else ""
        return VenvPaths(
            root=venv_dir,
            binroot=binroot,
            python=binroot / f"python{suffix}",
            toolr=binroot / f"toolr{suffix}",
        )

    return _make


def wheel_namelist(wheel: Path) -> list[str]:
    with ZipFile(wheel) as zf:
        return sorted(zf.namelist())
