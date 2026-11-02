"""Check that CI dogfoods the same toolr version as ``mise.toml`` and ``tools/uv.lock``.

The binary emits a runner spec that the ``toolr-py`` in the tools venv must accept. ``TOOLR_VERSION`` in
``.github/env`` pins the binary every in-repo ``uses: ./`` call installs; Renovate's "toolr release" group
bumps it with the other two pins.

The action installs ``latest`` when ``version`` is empty, so checking the expression text isn't enough: the
pin has to reach the step. A step that reads ``env.TOOLR_VERSION`` needs ``.github/env`` loaded earlier in its
job, and every call needs an earlier guard step that fails on an empty ``TOOLR_VERSION``.
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import tomllib
from pathlib import Path
from typing import Any
from typing import NamedTuple

import pytest
import yaml

REPO_ROOT = Path(__file__).resolve().parents[1]
GITHUB_DIR = REPO_ROOT / ".github"
ENV_PIN = "${{ env.TOOLR_VERSION }}"
INPUT_PIN = "${{ inputs.toolr-version }}"
CALLER_PIN = "${{ needs.prepare-ci.outputs.toolr-version }}"
EXPOSE_OUTPUT = "${{ steps.toolr-version.outputs.toolr-version }}"
LOADS_GITHUB_ENV = re.compile(r"\.github/env\b.*>>\s*\"?\$GITHUB_ENV")
GUARD = 'test -n "$TOOLR_VERSION"'


class StepList(NamedTuple):
    """The steps of one workflow job or composite action, and the file's declared inputs."""

    where: str
    steps: list[dict[str, Any]]
    inputs: dict[str, Any]


def _load(path: Path) -> dict[Any, Any]:
    return yaml.safe_load(path.read_text(encoding="utf-8"))


def _workflow_paths() -> list[Path]:
    return sorted((GITHUB_DIR / "workflows").glob("*.y*ml"))


def _workflow_call_inputs(data: dict[Any, Any]) -> dict[str, Any]:
    # PyYAML reads the bare `on` key as the boolean True.
    triggers = data.get(True) or {}
    if not isinstance(triggers, dict):
        return {}
    return (triggers.get("workflow_call") or {}).get("inputs") or {}


def _step_lists() -> list[StepList]:
    step_lists: list[StepList] = []
    for path in _workflow_paths():
        data = _load(path)
        inputs = _workflow_call_inputs(data)
        step_lists.extend(
            StepList(f"{path.name}:{job_id}", job.get("steps") or [], inputs)
            for job_id, job in (data.get("jobs") or {}).items()
        )
    for path in sorted((GITHUB_DIR / "actions").glob("*/action.y*ml")):
        data = _load(path)
        step_lists.append(
            StepList(
                str(path.relative_to(GITHUB_DIR)),
                (data.get("runs") or {}).get("steps") or [],
                data.get("inputs") or {},
            )
        )
    return step_lists


def _loads_github_env(step: dict[str, Any]) -> bool:
    return bool(LOADS_GITHUB_ENV.search(step.get("run") or ""))


def _is_guard(step: dict[str, Any], expected_env: str | None) -> bool:
    # A conditional or non-fatal guard can't stop the call.
    return (
        GUARD in (step.get("run") or "")
        and "if" not in step
        and not step.get("continue-on-error")
        and (step.get("env") or {}).get("TOOLR_VERSION") == expected_env
    )


def _call_offences(step_list: StepList) -> list[str]:
    offences = []
    for index, step in enumerate(step_list.steps):
        if step.get("uses") != "./":
            continue
        earlier = step_list.steps[:index]
        version = (step.get("with") or {}).get("version")
        if version == ENV_PIN:
            if not any(_loads_github_env(prior) for prior in earlier):
                offences.append(
                    f"{step_list.where}: reads {ENV_PIN} without loading .github/env first"
                )
            guard_env = None
        elif version == INPUT_PIN:
            if (step_list.inputs.get("toolr-version") or {}).get("required") is not True:
                offences.append(
                    f"{step_list.where}: reads {INPUT_PIN} without a required toolr-version input"
                )
            guard_env = INPUT_PIN
        else:
            offences.append(f"{step_list.where}: passes version {version!r}")
            continue
        if "TOOLR_VERSION" in (step.get("env") or {}):
            offences.append(f"{step_list.where}: overrides TOOLR_VERSION on the `uses: ./` step")
        if not any(_is_guard(prior, guard_env) for prior in earlier):
            offences.append(f"{step_list.where}: no earlier `{GUARD}` guard step")
    return offences


def _github_env_pin() -> str:
    for line in (GITHUB_DIR / "env").read_text(encoding="utf-8").splitlines():
        if line.startswith("TOOLR_VERSION="):
            value = line.removeprefix("TOOLR_VERSION=").strip()
            assert value, "TOOLR_VERSION in .github/env is empty"
            return value
    msg = "TOOLR_VERSION is missing from .github/env"
    raise AssertionError(msg)


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


def test_in_repo_action_calls_get_the_pin() -> None:
    step_lists = _step_lists()
    assert any(step.get("uses") == "./" for step_list in step_lists for step in step_list.steps), (
        "found no `uses: ./` steps under .github"
    )
    offences = [offence for step_list in step_lists for offence in _call_offences(step_list)]
    assert not offences, "\n".join(offences)


def test_reusable_workflow_callers_pass_the_loaded_pin() -> None:
    workflows = {path.name: _load(path) for path in _workflow_paths()}
    reusable = [
        name for name, data in workflows.items() if "toolr-version" in _workflow_call_inputs(data)
    ]
    assert reusable, "no reusable workflow takes a toolr-version input"
    offences = []
    for name in reusable:
        callers = [
            (caller, job_id, job)
            for caller, data in workflows.items()
            for job_id, job in (data.get("jobs") or {}).items()
            if job.get("uses") == f"./.github/workflows/{name}"
        ]
        if not callers:
            offences.append(f"nothing calls {name}")
        for caller, job_id, job in callers:
            where = f"{caller}:{job_id}"
            if (job.get("with") or {}).get("toolr-version") != CALLER_PIN:
                offences.append(f"{where}: doesn't pass toolr-version: {CALLER_PIN}")
            if "prepare-ci" not in (job.get("needs") or []):
                offences.append(f"{where}: doesn't need prepare-ci")
            prepare_ci = workflows[caller]["jobs"].get("prepare-ci") or {}
            if (prepare_ci.get("outputs") or {}).get("toolr-version") != EXPOSE_OUTPUT:
                offences.append(f"{caller}:prepare-ci: toolr-version output isn't {EXPOSE_OUTPUT}")
            steps = prepare_ci.get("steps") or []
            expose = [i for i, step in enumerate(steps) if step.get("id") == "toolr-version"]
            if not expose or "toolr-version=$TOOLR_VERSION" not in (
                steps[expose[0]].get("run") or ""
            ):
                offences.append(
                    f"{caller}:prepare-ci: no `toolr-version` step exposing $TOOLR_VERSION"
                )
            elif not any(_loads_github_env(step) for step in steps[: expose[0]]):
                offences.append(
                    f"{caller}:prepare-ci: exposes toolr-version before loading .github/env"
                )
    assert not offences, "\n".join(offences)


def _guard_scripts() -> list[str]:
    return sorted(
        {
            step["run"]
            for step_list in _step_lists()
            for step in step_list.steps
            if GUARD in (step.get("run") or "")
        }
    )


@pytest.mark.parametrize("script", _guard_scripts())
@pytest.mark.parametrize(("value", "passes"), [("", False), ("0.35.1", True)])
def test_guard_fails_on_an_empty_pin(script: str, value: str, passes: bool) -> None:
    bash = shutil.which("bash")
    if bash is None:
        pytest.skip("bash not available; the guard steps only run on ubuntu runners")
    result = subprocess.run(  # noqa: S603  # bash + script come from this repo's workflows
        [bash, "-e", "-c", script],
        env={**os.environ, "TOOLR_VERSION": value},
        capture_output=True,
        text=True,
        check=False,
    )
    assert (result.returncode == 0) is passes, result
