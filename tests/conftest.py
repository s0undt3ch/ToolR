from __future__ import annotations

import dataclasses
import importlib
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
from types import ModuleType

import pytest
from packaging.version import Version

from toolr._decorators import _get_command_group_storage
from toolr.testing import CommandsTester

pytest_plugins = ["pytester"]

_REPO_ROOT = Path(__file__).resolve().parent.parent


@pytest.fixture(autouse=True)
def _clean_registry() -> Iterator[None]:
    """Give every test an empty process-wide command-group registry.

    Snapshots the storage, clears it, and restores the snapshot afterwards,
    so a ``command_group`` call in one test can't be returned by the next.
    ``CommandsTester`` patches the storage function per test and is
    unaffected: this fixture sets up first and tears down last.
    """
    storage = _get_command_group_storage()
    saved = dict(storage)
    storage.clear()
    yield
    storage.clear()
    storage.update(saved)


@pytest.fixture(autouse=True)
def _restore_cwd() -> Iterator[None]:
    """Put the process back in its starting directory after every test.

    Guards against tests, and the code they call, that `os.chdir` without `monkeypatch`, so test
    authors need no chdir cleanup of their own.
    """
    original = os.getcwd()
    yield
    os.chdir(original)


@pytest.fixture(scope="session")
def _uv_cache_dir() -> str | None:
    """The real uv cache to pin, resolved before any test moves `XDG_CACHE_HOME`.

    `None` means leave uv alone: `UV_CACHE_DIR` is already set, or uv isn't installed.
    """
    if "UV_CACHE_DIR" in os.environ:
        return None
    uv = shutil.which("uv")
    if uv is None:
        return None
    proc = subprocess.run([uv, "cache", "dir"], capture_output=True, text=True, check=False)  # noqa: S603
    return proc.stdout.strip() or None


@pytest.fixture(autouse=True)
def _isolate_toolr_cache(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path_factory: pytest.TempPathFactory,
    _uv_cache_dir: str | None,
) -> None:
    """Keep every toolr a test spawns out of the user's real cache dir.

    uv also reads `XDG_CACHE_HOME`, so pin it to its real cache to keep it warm.
    """
    if _uv_cache_dir is not None:
        monkeypatch.setenv("UV_CACHE_DIR", _uv_cache_dir)
    monkeypatch.setenv("XDG_CACHE_HOME", str(tmp_path_factory.mktemp("xdg-cache")))


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
            # toolr writes UTF-8; Windows would otherwise decode with the ANSI code page.
            encoding="utf-8",
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
            "TOOLR_NO_CACHE_HINT": "1",
            "TOOLR_TRUST_ALL": "1",
        }
        return ToolsProject(root=root, toolr_bin=toolr_bin, env=env)

    return _make


@pytest.fixture
def make_tools_module(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> Iterator[Callable[..., ModuleType]]:
    """Factory: write a module (optionally inside a package) under `tmp_path` and import it.

    Extra package files go in `files` as `{relative path: source}`. Every
    module imported from `tmp_path` leaves `sys.modules` on teardown.
    """
    root = tmp_path / "modules"
    root.mkdir()
    monkeypatch.syspath_prepend(str(root))

    def _make(
        source: str,
        name: str = "tc_mod",
        package: str | None = None,
        files: dict[str, str] | None = None,
    ) -> ModuleType:
        base = root
        if package is not None:
            base = root / package
            base.mkdir(exist_ok=True)
            (base / "__init__.py").write_text("")
        for rel, body in (files or {}).items():
            (base / rel).write_text(textwrap.dedent(body))
        (base / f"{name}.py").write_text(textwrap.dedent(source))
        importlib.invalidate_caches()
        return importlib.import_module(name if package is None else f"{package}.{name}")

    yield _make
    for mod_name, mod in list(sys.modules.items()):
        if str(getattr(mod, "__file__", None) or "").startswith(str(root)):
            del sys.modules[mod_name]
