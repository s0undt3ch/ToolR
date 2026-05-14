"""End-to-end: install both wheels into a fresh venv and run a real command."""

from __future__ import annotations

import shutil
import subprocess
import sys
from pathlib import Path

import pytest


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

    subprocess.run(  # noqa: S603
        [str(python), "-m", "pip", "install", str(toolr_wheel), str(toolr_py_wheel)],
        check=True,
    )

    assert toolr.exists(), f"toolr binary not installed at {toolr}"

    result = subprocess.run(  # noqa: S603
        [str(toolr), "--version"],
        capture_output=True,
        text=True,
        check=True,
    )
    assert "0.20.0" in result.stdout, f"unexpected --version output: {result.stdout!r}"

    result = subprocess.run(  # noqa: S603
        [str(python), "-c", "import toolr; import toolr.utils._rust_utils; print(toolr.__version__)"],
        capture_output=True,
        text=True,
        check=True,
    )
    assert "0.20.0" in result.stdout
