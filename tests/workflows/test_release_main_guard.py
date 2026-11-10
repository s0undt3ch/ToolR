"""Tests for the main-branch guard that opens `release.yml`'s first job."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest
import yaml

from tests.workflows.conftest import StepRunner

REPO_ROOT = Path(__file__).resolve().parents[2]
RELEASE_YML = REPO_ROOT / ".github" / "workflows" / "release.yml"

pytestmark = pytest.mark.skip_on_windows(reason="the step runs under bash on ubuntu-latest only")


@pytest.fixture
def guard_step() -> dict[str, Any]:
    data = yaml.safe_load(RELEASE_YML.read_text(encoding="utf-8"))
    return data["jobs"]["prepare-ci"]["steps"][0]


def test_guard_is_the_first_step_and_skips_main(guard_step: dict[str, Any]) -> None:
    assert guard_step["name"] == "Require the main branch"
    assert guard_step["if"] == "${{ github.ref != 'refs/heads/main' }}"


@pytest.mark.parametrize("ref", ["refs/heads/feature", "refs/tags/v1.0.0"])
def test_guard_fails_for_any_other_ref(
    guard_step: dict[str, Any],
    bash_step: Callable[[str], StepRunner],
    tmp_path: Path,
    ref: str,
) -> None:
    result = bash_step(guard_step["run"])(tmp_path, {"REF": ref})
    assert result.returncode == 1
    assert ref in result.stderr
