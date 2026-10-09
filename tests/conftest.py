from __future__ import annotations

import dataclasses
import importlib.metadata
import json
import os
import shutil
import subprocess
import sys
import textwrap
from collections.abc import Callable
from collections.abc import Iterator
from pathlib import Path

import pytest
from packaging.version import Version

from toolr.testing import CommandsTester

pytest_plugins = ["pytester"]

# --------------------------------------------------------------------
# Subprocess-coverage bootstrap.
# --------------------------------------------------------------------
#
# CI's `_test.yml` exports `PYTHONPATH=tests/support/coverage` +
# `COVERAGE_PROCESS_START=.coveragerc` before invoking pytest so that
# subprocess Pythons spawned by tests (`python -m toolr._runner`,
# the cross-wheel install smoke, …) run
# `sitecustomize.py` → `coverage.process_startup()` and contribute
# data files to the parallel coverage run.
#
# Locally, `pytest` ran without those exports silently loses every
# subprocess's coverage credit. Set them here so the local invocation
# mirrors CI without each contributor needing to remember the wrapper
# env vars. The shim is a no-op when coverage isn't active (the
# sitecustomize swallows `ImportError`).

_TESTS_DIR = Path(__file__).resolve().parent
_REPO_ROOT = _TESTS_DIR.parent
_COVERAGE_SUPPORT_DIR = _TESTS_DIR / "support" / "coverage"
_COVERAGERC = _REPO_ROOT / ".coveragerc"


def pytest_configure(config: pytest.Config) -> None:
    """Match CI's `_test.yml` env for subprocess coverage."""
    if _COVERAGE_SUPPORT_DIR.is_dir():
        existing = os.environ.get("PYTHONPATH", "")
        entries = existing.split(os.pathsep) if existing else []
        if str(_COVERAGE_SUPPORT_DIR) not in entries:
            os.environ["PYTHONPATH"] = (
                str(_COVERAGE_SUPPORT_DIR) + os.pathsep + existing
                if existing
                else str(_COVERAGE_SUPPORT_DIR)
            )
    if _COVERAGERC.is_file():
        os.environ.setdefault("COVERAGE_PROCESS_START", str(_COVERAGERC))


@pytest.fixture
def commands_tester(tmp_path: Path) -> Iterator[CommandsTester]:
    """Create a commands tester."""
    commands_tester = CommandsTester(search_path=tmp_path)
    with commands_tester:
        commands_tester.discover()
        yield commands_tester


def _build_checkout_toolr(config: pytest.Config) -> Path:
    """`cargo build -p toolr` and return the built executable's path."""
    cargo = shutil.which("cargo")
    if cargo is None:
        pytest.skip("cargo not on PATH — can't build the checkout's toolr binary")
    reporter = config.pluginmanager.get_plugin("terminalreporter")
    if reporter is not None:
        reporter.write_line("building toolr from checkout (cargo build -p toolr)…")
    proc = subprocess.run(  # noqa: S603
        [cargo, "build", "-p", "toolr", "--message-format=json-render-diagnostics"],
        cwd=_REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        pytest.fail(f"`cargo build -p toolr` failed:\n{proc.stderr}")
    for line in proc.stdout.splitlines():
        message = json.loads(line)
        if message.get("reason") != "compiler-artifact" or message["target"]["name"] != "toolr":
            continue
        if message.get("executable"):
            return Path(message["executable"])
    pytest.fail("`cargo build -p toolr` reported no toolr executable")


@pytest.fixture(scope="session")
def toolr_bin(request: pytest.FixtureRequest) -> Path:
    """The `toolr` binary for subprocess tests, version-matched to the importable `toolr-py`.

    CI puts its prebuilt binary on PATH. Locally PATH can't be trusted: the
    workspace venv carries the last release's `toolr` from PyPI, so build
    the checkout's binary instead.
    """
    if os.environ.get("GITHUB_ACTIONS"):
        found = shutil.which("toolr")
        if found is None:
            pytest.fail("no toolr binary on PATH — CI must put the prebuilt binary there")
        binary = Path(found)
    else:
        binary = _build_checkout_toolr(request.config)
    proc = subprocess.run(  # noqa: S603
        [str(binary), "--version"], capture_output=True, text=True, check=True
    )
    binary_version = proc.stdout.split()[-1]
    toolr_py_version = importlib.metadata.version("toolr-py")
    # Cargo's SemVer (`0.34.1-dev25`) and PEP 440 (`0.34.1.dev25`) spell the same version differently.
    if Version(binary_version) != Version(toolr_py_version):
        pytest.fail(
            f"{binary} is toolr {binary_version} but the importable toolr-py is "
            f"{toolr_py_version}; subprocess tests would mix two releases"
        )
    return binary


@dataclasses.dataclass(frozen=True)
class ToolsProject:
    """A throwaway repo with a `tools/` package, driven through the real binary."""

    root: Path
    toolr_bin: Path
    env: dict[str, str]

    def run(self, *args: str) -> subprocess.CompletedProcess[str]:
        """Run `toolr *args` in the project root."""
        return subprocess.run(  # noqa: S603
            [str(self.toolr_bin), *args],
            cwd=self.root,
            env=self.env,
            capture_output=True,
            text=True,
            check=False,
        )


@pytest.fixture
def make_tools_project(tmp_path: Path, toolr_bin: Path) -> Callable[..., ToolsProject]:
    """Factory: write `tools/<name>.py` modules into a fresh project.

    `tools/.venv` links to the workspace venv, which already has the
    checkout's (or CI's prebuilt) `toolr-py`, so no `uv sync` is needed.
    """

    def _make(**modules: str) -> ToolsProject:
        root = tmp_path / "project"
        tools = root / "tools"
        tools.mkdir(parents=True)
        (tools / "__init__.py").write_text("")
        (tools / "pyproject.toml").write_text(
            '[project]\nname = "e2e-tools"\nversion = "0"\n\n[tool.toolr]\nvenv-location = "in-tree"\n'
        )
        for name, body in modules.items():
            (tools / f"{name}.py").write_text(textwrap.dedent(body))
        (tools / ".venv").symlink_to(Path(sys.prefix), target_is_directory=True)
        env = {key: value for key, value in os.environ.items() if not key.startswith("TOOLR_")}
        env |= {
            "PATH": str(toolr_bin.parent) + os.pathsep + env.get("PATH", ""),
            "XDG_CACHE_HOME": str(tmp_path / "cache"),
            "TOOLR_NO_CACHE_HINT": "1",
        }
        return ToolsProject(root=root, toolr_bin=toolr_bin, env=env)

    return _make
