"""Tests for the step of `release.yml` that removes the Test PyPI attestations.

`pypa/gh-action-pypi-publish` writes a `*.publish.attestation` file into `dist/`
for each file it uploads. Left there, the GitHub release uploads them as assets
and the PyPI step refuses to overwrite them, which failed the 0.36.0 release.
"""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

import pytest
import yaml

from tests.workflows.conftest import StepRunner
from tests.workflows.conftest import workflow_step_script

REPO_ROOT = Path(__file__).resolve().parents[2]
RELEASE_YML = REPO_ROOT / ".github" / "workflows" / "release.yml"
STEP_ID = "remove-test-pypi-attestations"


def _step_names() -> list[str]:
    data = yaml.safe_load(RELEASE_YML.read_text(encoding="utf-8"))
    return [step.get("name", "") for step in data["jobs"]["publish-release"]["steps"]]


def test_runs_between_test_pypi_and_every_later_use_of_dist() -> None:
    names = _step_names()
    test_pypi = names.index("Publish to Test PyPI (Trusted Publisher)")
    assert names[test_pypi + 1] == "Remove the Test PyPI attestations"
    assert test_pypi + 1 < names.index("Create GitHub release")
    assert test_pypi + 1 < names.index("Publish to PyPI (Trusted Publisher)")


@pytest.mark.skip_on_windows(reason="the step runs under bash on ubuntu-latest only")
def test_removes_only_attestations(tmp_path: Path, bash_step: Callable[[str], StepRunner]) -> None:
    step = bash_step(workflow_step_script(RELEASE_YML, "publish-release", STEP_ID))
    dist = tmp_path / "dist"
    dist.mkdir()
    keep = ["toolr-1.0.0-py3-none-any.whl", "toolr-1.0.0.tar.gz"]
    for name in keep:
        (dist / name).write_text("")
        (dist / f"{name}.publish.attestation").write_text("{}")
    result = step(tmp_path, {})
    assert result.returncode == 0, result.stderr
    assert sorted(p.name for p in dist.iterdir()) == sorted(keep)
