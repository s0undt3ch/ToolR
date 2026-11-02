"""Check that CI dogfoods the same toolr version as ``mise.toml`` and ``tools/uv.lock``.

The binary emits a runner spec that the ``toolr-py`` in the tools venv must accept. ``TOOLR_VERSION`` in
``.github/env`` pins the binary every in-repo ``uses: ./`` call installs; Renovate's "toolr release" group
bumps it with the other two pins.
"""

from __future__ import annotations

import tomllib
from pathlib import Path
from typing import Any

import yaml

REPO_ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS_DIR = REPO_ROOT / ".github" / "workflows"
PREPARE_RELEASE = "_prepare-release.yml"
ENV_PIN = "${{ env.TOOLR_VERSION }}"
INPUT_PIN = "${{ inputs.toolr-version }}"
CALLER_PIN = "${{ needs.prepare-ci.outputs.toolr-version }}"


def _load_workflow(name: str) -> dict[Any, Any]:
    return yaml.safe_load((WORKFLOWS_DIR / name).read_text(encoding="utf-8"))


def _github_env_pin() -> str:
    for line in (REPO_ROOT / ".github" / "env").read_text(encoding="utf-8").splitlines():
        if line.startswith("TOOLR_VERSION="):
            value = line.removeprefix("TOOLR_VERSION=").strip()
            # The action falls back to `latest` on an empty version.
            assert value, "TOOLR_VERSION in .github/env is empty"
            return value
    msg = "TOOLR_VERSION is missing from .github/env"
    raise AssertionError(msg)


def _in_repo_action_calls() -> list[tuple[str, str, dict[str, Any]]]:
    calls: list[tuple[str, str, dict[str, Any]]] = []
    for path in sorted(WORKFLOWS_DIR.glob("*.yml")):
        data = yaml.safe_load(path.read_text(encoding="utf-8"))
        for job_id, job in (data.get("jobs") or {}).items():
            calls.extend(
                (path.name, job_id, step)
                for step in job.get("steps") or []
                if step.get("uses") == "./"
            )
    return calls


def test_toolr_pin_matches_mise_and_lock() -> None:
    mise = tomllib.loads((REPO_ROOT / "mise.toml").read_text(encoding="utf-8"))
    lock = tomllib.loads((REPO_ROOT / "tools" / "uv.lock").read_text(encoding="utf-8"))
    locked = [package["version"] for package in lock["package"] if package["name"] == "toolr-py"]
    assert locked, "tools/uv.lock has no toolr-py package"
    pins = {
        ".github/env TOOLR_VERSION": _github_env_pin(),
        "mise.toml aqua:s0undt3ch/ToolR": mise["tools"]["aqua:s0undt3ch/ToolR"],
        "tools/uv.lock toolr-py": locked[0],
    }
    assert len(set(pins.values())) == 1, pins


def test_in_repo_action_calls_use_the_pin() -> None:
    calls = _in_repo_action_calls()
    assert calls, "found no `uses: ./` steps under .github/workflows"
    offenders = [
        f"{workflow}:{job_id} passes version {step.get('with', {}).get('version')!r}"
        for workflow, job_id, step in calls
        if step.get("with", {}).get("version")
        != (INPUT_PIN if workflow == PREPARE_RELEASE else ENV_PIN)
    ]
    assert not offenders, "\n".join(offenders)


def test_prepare_release_requires_toolr_version() -> None:
    # PyYAML reads the bare `on` key as the boolean True.
    inputs = _load_workflow(PREPARE_RELEASE)[True]["workflow_call"]["inputs"]
    assert inputs["toolr-version"]["required"] is True
    for caller in ("ci.yml", "release.yml"):
        jobs = _load_workflow(caller)["jobs"]
        callers = [
            job
            for job in jobs.values()
            if job.get("uses") == f"./.github/workflows/{PREPARE_RELEASE}"
        ]
        assert callers, f"{caller} doesn't call {PREPARE_RELEASE}"
        for job in callers:
            assert job["with"].get("toolr-version") == CALLER_PIN, caller
