"""Smoke tests for `mise use aqua:s0undt3ch/ToolR@<version>`.

Catches breakage outside this repo: aqua-registry dropping or changing the
ToolR entry, mise's aqua backend changing, or the release archive layout
drifting from what the registry entry expects.
"""

from __future__ import annotations

from tests.distribution.mise.conftest import MiseProject


def test_toolr_runs_at_release_version(aqua_project: MiseProject, smoke_version: str) -> None:
    result = aqua_project.run_toolr("--version")
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == f"toolr {smoke_version}"
