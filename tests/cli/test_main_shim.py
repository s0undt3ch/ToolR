"""Tests for the `python -m toolr` deprecation shim."""

from __future__ import annotations

import os
import stat
import subprocess
import sys
import textwrap
from collections.abc import Callable
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]
PY_SRC = REPO_ROOT / "python"


@pytest.fixture
def fake_binary(tmp_path: Path) -> Callable[..., Path]:
    """Factory: write a fake ``toolr`` binary under ``tmp_path/bin``."""

    def _make(exit_code: int = 0) -> Path:
        bin_dir = tmp_path / "bin"
        bin_dir.mkdir()
        binary = bin_dir / ("toolr.exe" if os.name == "nt" else "toolr")
        if os.name == "nt":
            binary.write_text(f'import sys\nprint("FAKE-TOOLR", " ".join(sys.argv[1:]))\nsys.exit({exit_code})\n')
        else:
            binary.write_text(
                textwrap.dedent(
                    f"""\
                    #!{sys.executable}
                    import sys
                    print("FAKE-TOOLR", " ".join(sys.argv[1:]))
                    sys.exit({exit_code})
                    """
                )
            )
            binary.chmod(binary.stat().st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH)
        return binary

    return _make


@pytest.mark.skipif(os.name == "nt", reason="POSIX exec semantics required")
def test_shim_execs_real_toolr_with_argv(fake_binary: Callable[..., Path]) -> None:
    fake = fake_binary()
    env = {
        **os.environ,
        "PATH": str(fake.parent) + os.pathsep + os.environ.get("PATH", ""),
        "PYTHONPATH": str(PY_SRC),
        "TOOLR_NO_DEPRECATION_NOTICE": "1",
        "TOOLR_SHIM_DISABLE_INTERPRETER_BIN": "1",
    }
    result = subprocess.run(  # noqa: S603
        [sys.executable, "-m", "toolr", "ci", "--help"],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )
    assert "FAKE-TOOLR ci --help" in result.stdout, result.stdout


@pytest.mark.skipif(os.name == "nt", reason="POSIX exec semantics required")
def test_shim_prints_deprecation_notice(fake_binary: Callable[..., Path]) -> None:
    fake = fake_binary()
    env = {
        **os.environ,
        "PATH": str(fake.parent) + os.pathsep + os.environ.get("PATH", ""),
        "PYTHONPATH": str(PY_SRC),
        "TOOLR_SHIM_DISABLE_INTERPRETER_BIN": "1",
    }
    env.pop("TOOLR_NO_DEPRECATION_NOTICE", None)
    result = subprocess.run(  # noqa: S603
        [sys.executable, "-W", "always::DeprecationWarning", "-m", "toolr", "--version"],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )
    assert "DeprecationWarning" in result.stderr or "deprecated" in result.stderr.lower()


@pytest.mark.skipif(os.name == "nt", reason="POSIX exec semantics required")
def test_shim_suppresses_notice_when_env_set(fake_binary: Callable[..., Path]) -> None:
    fake = fake_binary()
    env = {
        **os.environ,
        "PATH": str(fake.parent) + os.pathsep + os.environ.get("PATH", ""),
        "PYTHONPATH": str(PY_SRC),
        "TOOLR_NO_DEPRECATION_NOTICE": "1",
        "TOOLR_SHIM_DISABLE_INTERPRETER_BIN": "1",
    }
    result = subprocess.run(  # noqa: S603
        [sys.executable, "-m", "toolr", "--version"],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )
    assert "deprecated" not in result.stderr.lower(), result.stderr


def test_shim_errors_when_binary_missing(tmp_path: Path) -> None:
    empty = tmp_path / "empty"
    empty.mkdir()
    env = {
        **os.environ,
        "PATH": str(empty),
        "PYTHONPATH": str(PY_SRC),
        "TOOLR_NO_DEPRECATION_NOTICE": "1",
        "TOOLR_SHIM_DISABLE_INTERPRETER_BIN": "1",
    }
    result = subprocess.run(  # noqa: S603
        [sys.executable, "-m", "toolr", "--version"],
        check=False,
        capture_output=True,
        text=True,
        env=env,
    )
    assert result.returncode != 0
    assert "toolr binary not found" in result.stderr.lower() or "binary not found" in result.stderr.lower(), (
        result.stderr
    )
