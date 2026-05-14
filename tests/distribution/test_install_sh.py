"""Tests for `installation/install.sh`."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

pytestmark = pytest.mark.skipif(
    sys.platform == "win32",
    reason="install.sh is POSIX-only",
)

REPO_ROOT = Path(__file__).resolve().parents[2]
INSTALL_SH = REPO_ROOT / "installation" / "install.sh"


def _sh() -> str:
    sh = shutil.which("sh")
    assert sh is not None, "sh missing"
    return sh


def test_install_sh_dry_run_runs_and_exits_zero() -> None:
    result = subprocess.run(  # noqa: S603
        [
            _sh(),
            str(INSTALL_SH),
            "--dry-run",
            "--version",
            "9.9.9",
            "--triple",
            "x86_64-unknown-linux-gnu",
        ],
        check=False,
        capture_output=True,
        text=True,
        env={**os.environ, "TOOLR_REPO": "s0undt3ch/ToolR"},
    )
    assert result.returncode == 0, result.stderr
    assert "version: 9.9.9" in result.stderr
    assert "triple:  x86_64-unknown-linux-gnu" in result.stderr


def test_install_sh_detects_host_triple_on_dry_run() -> None:
    result = subprocess.run(  # noqa: S603
        [_sh(), str(INSTALL_SH), "--dry-run", "--version", "0.0.0"],
        check=False,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, result.stderr
    # Auto-detected triple includes a known suffix.
    assert any(
        triple_fragment in result.stderr
        for triple_fragment in (
            "apple-darwin",
            "unknown-linux-gnu",
            "unknown-linux-musl",
        )
    ), result.stderr
