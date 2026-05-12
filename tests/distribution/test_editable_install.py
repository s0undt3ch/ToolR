"""Verify `pip install -e .` (via maturin develop) produces a working toolr."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

pytestmark = pytest.mark.skipif(
    shutil.which("uv") is None,
    reason="uv not on PATH",
)

REPO_ROOT = Path(__file__).resolve().parents[2]


@pytest.mark.xfail(
    reason=(
        "maturin in pyo3-bindings mode does not install [[bin]] targets "
        "from maturin develop. The standalone toolr binary ships via "
        "install.sh, mise, and GitHub release archives."
    ),
    strict=False,
)
def test_editable_install_yields_runnable_toolr_binary(tmp_path: Path) -> None:
    uv = shutil.which("uv")
    assert uv is not None
    venv_dir = tmp_path / "venv"
    subprocess.run(  # noqa: S603
        [uv, "venv", "--python", sys.executable, str(venv_dir)],
        check=True,
    )
    venv_bin = venv_dir / ("Scripts" if os.name == "nt" else "bin")
    env = {**os.environ, "VIRTUAL_ENV": str(venv_dir)}
    subprocess.run(  # noqa: S603
        [
            str(venv_bin / "python"),
            "-m",
            "pip",
            "install",
            "maturin>=1.7,<2.0",
        ],
        check=True,
        env=env,
    )
    subprocess.run(  # noqa: S603
        [str(venv_bin / "maturin"), "develop", "--release"],
        cwd=REPO_ROOT,
        check=True,
        env=env,
    )
    toolr_bin = venv_bin / ("toolr.exe" if os.name == "nt" else "toolr")
    assert toolr_bin.exists(), f"expected {toolr_bin} to exist after develop"
    result = subprocess.run(  # noqa: S603
        [str(toolr_bin), "--version"],
        check=True,
        capture_output=True,
        text=True,
    )
    assert "toolr" in result.stdout.lower(), result.stdout
