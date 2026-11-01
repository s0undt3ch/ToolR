"""Tests for the `setup-toolr` composite action's tools-venv cache key."""

from __future__ import annotations

import re
from pathlib import Path

import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
ACTION_YML = REPO_ROOT / "action.yml"

_HASH_FILES = re.compile(r"hashFiles\((.*?)\)")
_QUOTED = re.compile(r"'([^']+)'")


def _step(step_id: str) -> dict[str, object]:
    """Return the action.yml step with the given `id`."""
    data = yaml.safe_load(ACTION_YML.read_text())
    for step in data["runs"]["steps"]:
        if step.get("id") == step_id:
            return step
    msg = f"no `{step_id}` step in action.yml"
    raise AssertionError(msg)


def _venv_cache_key() -> str:
    with_ = _step("venv-cache")["with"]
    assert isinstance(with_, dict)
    return with_["key"]


def test_tools_venv_cache_key_hashes_only_tools_files() -> None:
    # The root `uv.lock` doesn't describe the tools venv, so a change to it
    # must not bust the cache.
    match = _HASH_FILES.search(_venv_cache_key())
    assert match is not None
    assert _QUOTED.findall(match.group(1)) == ["tools/pyproject.toml", "tools/uv.lock"]


def test_tools_venv_cache_key_keeps_prefix_and_triple() -> None:
    key = _venv_cache_key()
    assert key.startswith("${{ inputs.cache-prefix }}-tools-venv|")
    assert "${{ steps.archive.outputs.triple }}" in key
