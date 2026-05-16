"""End-to-end: install both wheels into a fresh venv and run a real command."""

from __future__ import annotations

import re
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parent.parent.parent

# Matches the wheel's PEP 427-style filename: `<name>-<version>-<tag>...whl`.
_WHEEL_VERSION_RE = re.compile(r"^[^-]+-(?P<version>[^-]+)-")


def _version_from_wheel(wheel: Path) -> str:
    """Extract the version from a wheel filename.

    Reading the workspace `Cargo.toml` would lie: the wheel is built from
    a sdist whose `Cargo.toml` was version-bumped by `_prepare-release.yml`
    (e.g. `0.11.2.dev262`), but the current checkout's `Cargo.toml` still
    pins the pre-bump version. The wheel filename is the source of truth
    for what version actually got built and shipped.
    """
    match = _WHEEL_VERSION_RE.match(wheel.name)
    if match is None:
        msg = f"could not parse version from wheel filename: {wheel.name!r}"
        raise ValueError(msg)
    return match.group("version")


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
    # `toolr --version` prints `toolr X.Y.Z[-dev...]`. cargo accepts both
    # `0.11.2-dev262` (semver) and `0.11.2.dev262` (PEP 440); the wheel
    # filename uses the PEP 440 dot-form, the binary may emit either,
    # so compare on the normalized PEP 440 form.
    binary_version = _version_from_wheel(toolr_wheel)
    assert binary_version.replace(".dev", "-dev") in result.stdout or binary_version in result.stdout, (
        f"unexpected --version output: {result.stdout!r} (expected to contain {binary_version!r})"
    )

    result = subprocess.run(  # noqa: S603
        [str(python), "-c", "import toolr; import toolr.utils._rust_utils; print(toolr.__version__)"],
        capture_output=True,
        text=True,
        check=True,
    )
    py_version = _version_from_wheel(toolr_py_wheel)
    assert py_version in result.stdout, (
        f"unexpected toolr.__version__ output: {result.stdout!r} (expected to contain {py_version!r})"
    )
