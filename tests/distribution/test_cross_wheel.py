"""End-to-end: install both wheels into a fresh venv and run a real command."""

from __future__ import annotations

import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent.parent


def _workspace_version() -> str:
    with (REPO_ROOT / "Cargo.toml").open("rb") as f:
        return tomllib.load(f)["workspace"]["package"]["version"]


@pytest.mark.distribution
def test_install_both_wheels_and_run_subcommand(
    toolr_wheel: Path,
    toolr_py_wheel: Path,
    tmp_path: Path,
) -> None:
    uv = shutil.which("uv")
    if uv is None:
        pytest.skip("uv not on PATH; cross-wheel install smoke needs uv venv to bootstrap pip")

    venv_dir = tmp_path / "smoke-venv"
    subprocess.run(  # noqa: S603
        [uv, "venv", "--seed", "--python", sys.executable, str(venv_dir)],
        check=True,
    )

    if sys.platform == "win32":
        python = venv_dir / "Scripts" / "python.exe"
        toolr = venv_dir / "Scripts" / "toolr.exe"
    else:
        python = venv_dir / "bin" / "python"
        toolr = venv_dir / "bin" / "toolr"

    # The conftest parametrizes both wheel fixtures over every wheel
    # currently in `wheelhouse/`, which in CI is "all wheels for this OS
    # family" (manylinux + musllinux + aarch64 + x86_64 + every CPython).
    # Most combinations are not installable on the current runner; pip
    # surfaces that as `is not a supported wheel on this platform`. Treat
    # those as a skip — the goal of the test is to assert the cross-wheel
    # install path works *for installable wheels*, not to verify pip's
    # platform-tag rejection logic.
    result = subprocess.run(  # noqa: S603
        [str(python), "-m", "pip", "install", str(toolr_wheel), str(toolr_py_wheel)],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        if "is not a supported wheel on this platform" in (result.stderr + result.stdout):
            pytest.skip(f"platform-incompatible wheel combo on this runner: {toolr_wheel.name} + {toolr_py_wheel.name}")
        pytest.fail(f"pip install failed:\n{result.stdout}\n{result.stderr}")

    assert toolr.exists(), f"toolr binary not installed at {toolr}"

    result = subprocess.run(  # noqa: S603
        [str(toolr), "--version"],
        capture_output=True,
        text=True,
        check=True,
    )
    version = _workspace_version()
    assert version in result.stdout, f"unexpected --version output: {result.stdout!r}"

    result = subprocess.run(  # noqa: S603
        [str(python), "-c", "import toolr; import toolr.utils._rust_utils; print(toolr.__version__)"],
        capture_output=True,
        text=True,
        check=True,
    )
    assert version in result.stdout
