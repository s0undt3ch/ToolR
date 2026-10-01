"""Fixtures for smoke tests against a *published* toolr release installed through mise.

These tests install from GitHub, not from `wheelhouse/`, so they only run when
`TOOLR_SMOKE_VERSION` names the release to check (`install-smoke.yml` sets it).
Without it, or without `mise` on `PATH`, every test here skips.

Each install runs against throwaway mise data, cache, state and config dirs, so
a local run never touches the developer's own mise installs. Set
`GITHUB_TOKEN`: mise fetches packslip skills through the GitHub API, and the
anonymous rate limit makes it drop skills while still reporting success.
"""

from __future__ import annotations

import base64
import dataclasses
import json
import os
import shutil
import subprocess
import urllib.request
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest

PACKSLIP_TOOL = "packslip:github.com/s0undt3ch/ToolR"
AQUA_TOOL = "aqua:s0undt3ch/ToolR"
PACKSLIP_BUNDLE_URL = (
    "https://github.com/s0undt3ch/ToolR/releases/download/v{version}/packslip.sigstore.json"
)
INSTALL_TIMEOUT = 600


@dataclasses.dataclass(frozen=True)
class MiseProject:
    """A scratch directory whose `mise.toml` pins one toolr install."""

    root: Path
    env: dict[str, str]
    mise: str

    def run(self, *args: str, timeout: float = 120) -> subprocess.CompletedProcess[str]:
        return subprocess.run(  # noqa: S603
            [self.mise, *args],
            cwd=self.root,
            env=self.env,
            capture_output=True,
            text=True,
            check=False,
            timeout=timeout,
        )

    def toolr_bin_dir(self) -> Path:
        result = self.run("which", "toolr")
        assert result.returncode == 0, result.stderr
        return Path(result.stdout.strip()).parent


@pytest.fixture(scope="session")
def smoke_version() -> str:
    version = os.environ.get("TOOLR_SMOKE_VERSION", "").strip().removeprefix("v")
    if not version:
        pytest.skip("TOOLR_SMOKE_VERSION not set; mise smoke tests need a published release")
    return version


@pytest.fixture(scope="session")
def mise_bin() -> str:
    mise = shutil.which("mise")
    if mise is None:
        pytest.skip("mise not on PATH")
    return mise


@pytest.fixture(scope="session")
def make_mise_project(
    tmp_path_factory: pytest.TempPathFactory,
    mise_bin: str,
) -> Callable[[str], MiseProject]:
    """Return a factory that runs `mise use <spec>` in a fresh, isolated project."""
    mise_home = tmp_path_factory.mktemp("mise-home")
    env = {key: value for key, value in os.environ.items() if not key.startswith("MISE_")}
    env.update(
        MISE_DATA_DIR=str(mise_home / "data"),
        MISE_CACHE_DIR=str(mise_home / "cache"),
        MISE_STATE_DIR=str(mise_home / "state"),
        MISE_CONFIG_DIR=str(mise_home / "config"),
        MISE_GLOBAL_CONFIG_FILE=str(mise_home / "config" / "config.toml"),
        MISE_YES="1",
    )

    def _make(spec: str) -> MiseProject:
        root = tmp_path_factory.mktemp("mise-project")
        project = MiseProject(
            root=root, env={**env, "MISE_CEILING_PATHS": str(root.parent)}, mise=mise_bin
        )
        result = project.run("use", spec, timeout=INSTALL_TIMEOUT)
        assert result.returncode == 0, (
            f"`mise use {spec}` failed:\n{result.stdout}\n{result.stderr}"
        )
        return project

    return _make


@pytest.fixture(scope="session")
def packslip_project(
    make_mise_project: Callable[[str], MiseProject], smoke_version: str
) -> MiseProject:
    return make_mise_project(f"{PACKSLIP_TOOL}@{smoke_version}")


@pytest.fixture(scope="session")
def aqua_project(
    make_mise_project: Callable[[str], MiseProject], smoke_version: str
) -> MiseProject:
    return make_mise_project(f"{AQUA_TOOL}@{smoke_version}")


@pytest.fixture(scope="session")
def packslip_manifest(smoke_version: str) -> dict[str, Any]:
    """The `predicate` of the release's signed packslip, i.e. what mise was promised.

    Signature verification is `release.yml`'s job; this only reads the declarations.
    """
    url = PACKSLIP_BUNDLE_URL.format(version=smoke_version)
    with urllib.request.urlopen(url, timeout=60) as response:  # noqa: S310
        bundle = json.load(response)
    statement = json.loads(base64.b64decode(bundle["dsseEnvelope"]["payload"]))
    predicate: dict[str, Any] = statement["predicate"]
    return predicate
