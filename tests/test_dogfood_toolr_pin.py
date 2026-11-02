"""Check that CI dogfoods the same toolr version as ``mise.toml`` and ``tools/uv.lock``.

The binary emits a runner spec that the ``toolr-py`` in the tools venv must accept. ``TOOLR_VERSION`` in
``.github/env`` pins the binary; ``.github/actions/setup-toolr`` reads it and is the only caller of the in-repo
Setup ToolR action. Renovate's "toolr release" group bumps all three pins together.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import tomllib
from pathlib import Path
from typing import Any

import pytest
import yaml

REPO_ROOT = Path(__file__).resolve().parents[1]
GITHUB_DIR = REPO_ROOT / ".github"
WRAPPER = ".github/actions/setup-toolr"
WRAPPER_PATH = REPO_ROOT / WRAPPER / "action.yml"


def _load(path: Path) -> dict[Any, Any]:
    return yaml.safe_load(path.read_text(encoding="utf-8"))


def _steps_by_file() -> dict[str, list[dict[str, Any]]]:
    steps: dict[str, list[dict[str, Any]]] = {}
    for path in sorted((GITHUB_DIR / "workflows").glob("*.y*ml")):
        for job in (_load(path).get("jobs") or {}).values():
            steps.setdefault(path.relative_to(REPO_ROOT).as_posix(), []).extend(
                job.get("steps") or []
            )
    for path in sorted((GITHUB_DIR / "actions").glob("*/action.y*ml")):
        steps[path.relative_to(REPO_ROOT).as_posix()] = (_load(path).get("runs") or {}).get(
            "steps"
        ) or []
    return steps


def _wrapper_pin_script() -> str:
    steps = _load(WRAPPER_PATH)["runs"]["steps"]
    scripts = [step["run"] for step in steps if ".github/env" in (step.get("run") or "")]
    assert len(scripts) == 1, (
        f"{WRAPPER} needs exactly one step that reads TOOLR_VERSION from .github/env"
    )
    return scripts[0]


def test_toolr_pin_matches_mise_and_lock() -> None:
    env = (GITHUB_DIR / "env").read_text(encoding="utf-8").splitlines()
    pinned = [
        line.removeprefix("TOOLR_VERSION=") for line in env if line.startswith("TOOLR_VERSION=")
    ]
    assert pinned, "add `TOOLR_VERSION=<version>` to .github/env"
    mise = tomllib.loads((REPO_ROOT / "mise.toml").read_text(encoding="utf-8"))
    lock = tomllib.loads((REPO_ROOT / "tools" / "uv.lock").read_text(encoding="utf-8"))
    locked = [package["version"] for package in lock["package"] if package["name"] == "toolr-py"]
    assert locked, "tools/uv.lock has no toolr-py package"
    pins = {
        ".github/env TOOLR_VERSION": pinned[0],
        "mise.toml aqua:s0undt3ch/ToolR": mise["tools"]["aqua:s0undt3ch/ToolR"],
        "tools/uv.lock toolr-py": locked[0],
    }
    assert len(set(pins.values())) == 1, f"set these to the same version: {pins}"


def test_setup_toolr_goes_through_the_wrapper() -> None:
    offences = []
    wrapper_calls = 0
    for where, steps in _steps_by_file().items():
        for step in steps:
            uses = step.get("uses")
            if uses == f"./{WRAPPER}":
                wrapper_calls += 1
            elif uses == "./" and where != f"{WRAPPER}/action.yml":
                offences.append(f"{where}: use `uses: ./{WRAPPER}` instead of `uses: ./`")
    assert not offences, "\n".join(offences)
    assert wrapper_calls, f"no workflow calls `uses: ./{WRAPPER}`"


def test_wrapper_passes_the_pin_to_setup_toolr() -> None:
    steps = _load(WRAPPER_PATH)["runs"]["steps"]
    calls = [step for step in steps if step.get("uses") == "./"]
    assert len(calls) == 1, f"{WRAPPER} must call `uses: ./` exactly once"
    assert calls[0]["with"]["version"] == "${{ steps.pin.outputs.version }}", (
        f"{WRAPPER} must pass `version: ${{{{ steps.pin.outputs.version }}}}` to `uses: ./`"
    )


@pytest.mark.parametrize(
    ("env_file", "expected"),
    [
        ("TOOLR_VERSION=9.9.9-test\n", "9.9.9-test"),
        ("TOOLR_VERSION=\n", None),
        ("MISE_VERSION=v1.0.0\n", None),
    ],
    ids=["set", "empty", "missing"],
)
def test_wrapper_reads_the_pin_and_fails_without_one(
    tmp_path: Path, env_file: str, expected: str | None
) -> None:
    bash = shutil.which("bash")
    if bash is None:
        pytest.skip("bash not available; the wrapper only runs on ubuntu runners")
    (tmp_path / ".github").mkdir()
    (tmp_path / ".github" / "env").write_text(env_file, encoding="utf-8")
    github_output = tmp_path / "github_output"
    github_output.touch()
    result = subprocess.run(  # noqa: S603  # bash + script come from this repo's wrapper action
        [bash, "-e", "-c", _wrapper_pin_script()],
        cwd=tmp_path,
        env={**os.environ, "GITHUB_OUTPUT": str(github_output)},
        capture_output=True,
        text=True,
        check=False,
    )
    if expected is None:
        assert result.returncode != 0, result
        assert "::error" in result.stdout, result
    else:
        assert result.returncode == 0, result
        assert github_output.read_text(encoding="utf-8") == f"version={expected}\n"
