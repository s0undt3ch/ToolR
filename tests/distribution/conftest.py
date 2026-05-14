"""Fixtures for distribution-shape tests."""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path
from zipfile import ZipFile

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent.parent


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
    out = tmp_path_factory.mktemp("toolr-wheel")
    return _build_wheel("crates/toolr/Cargo.toml", out)


@pytest.fixture(scope="session")
def toolr_py_wheel(tmp_path_factory: pytest.TempPathFactory) -> Path:
    out = tmp_path_factory.mktemp("toolr-py-wheel")
    return _build_wheel("crates/toolr-py/Cargo.toml", out)


def wheel_namelist(wheel: Path) -> list[str]:
    with ZipFile(wheel) as zf:
        return sorted(zf.namelist())
