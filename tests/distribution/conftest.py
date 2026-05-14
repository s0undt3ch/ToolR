"""Fixtures for distribution-shape tests."""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path
from zipfile import ZipFile

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent.parent


def _require_maturin() -> None:
    """Skip the test if ``maturin`` is not on PATH.

    The standard ``uv sync --dev`` test venv doesn't install maturin (it's a
    build-time tool, not a runtime dep). The wheel-shape tests are only
    meaningful in a wheel-build environment — install via
    ``uv tool install maturin`` or rely on the cibuildwheel-driven CI job that
    already has it on PATH.
    """
    if shutil.which("maturin") is None:
        pytest.skip(
            "maturin not on PATH; distribution tests require a wheel-build "
            "environment. Install via `uv tool install maturin` or run these "
            "tests under the cibuildwheel-driven CI job that already has it "
            "available.",
        )


def _build_wheel(manifest_relpath: str, out_dir: Path) -> Path:
    out_dir.mkdir(parents=True, exist_ok=True)
    subprocess.run(  # noqa: S603
        [
            shutil.which("maturin") or "maturin",
            "build",
            "--release",
            "-m",
            str(REPO_ROOT / manifest_relpath),
            "--out",
            str(out_dir),
        ],
        check=True,
    )
    wheels = list(out_dir.glob("*.whl"))
    assert len(wheels) == 1, f"expected 1 wheel in {out_dir}, found {wheels}"
    return wheels[0]


@pytest.fixture(scope="session")
def toolr_wheel(tmp_path_factory: pytest.TempPathFactory) -> Path:
    _require_maturin()
    out = tmp_path_factory.mktemp("toolr-wheel")
    return _build_wheel("crates/toolr/Cargo.toml", out)


@pytest.fixture(scope="session")
def toolr_py_wheel(tmp_path_factory: pytest.TempPathFactory) -> Path:
    _require_maturin()
    out = tmp_path_factory.mktemp("toolr-py-wheel")
    return _build_wheel("crates/toolr-py/Cargo.toml", out)


def wheel_namelist(wheel: Path) -> list[str]:
    with ZipFile(wheel) as zf:
        return sorted(zf.namelist())
