"""Tests for the `setup-toolr` composite action's resolve-version step.

The resolve-version step is a bash script embedded in `action.yml`.
This test extracts the script body, prepends a stubbed `gh` shell
function, and runs the result through `bash` for each scenario.

The action relies on action.yml's `inputs.version.default` being kept
in sync with the released version by `toolr version bump` — that
default is what makes SHA pins without a `version:` input resolve to
the matching binary (the bake-in mechanism added for #281). This test
covers the resolve script itself; the bake-in is tested in
`tests/tools/test_version.py`.
"""

from __future__ import annotations

import shutil
import subprocess
from collections.abc import Callable
from pathlib import Path

import pytest
import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
ACTION_YML = REPO_ROOT / "action.yml"


_MISSING_RESOLVE_STEP = "no `resolve-version` step in action.yml"

# In a consumer's workflow `GITHUB_REPOSITORY` is the consumer, which may
# have releases of its own. Its tag clears the `0.20.0` floor, so a lookup
# against it shows up as a wrong version, not a minimum-version error.
CALLER_REPO = "consumer/repo"
CALLER_TAG = "v9.9.9"


def _resolve_step_script() -> str:
    """Pull the resolve-version step's `run:` body out of action.yml."""
    data = yaml.safe_load(ACTION_YML.read_text())
    for step in data["runs"]["steps"]:
        if step.get("id") == "resolve-version":
            return step["run"]
    raise AssertionError(_MISSING_RESOLVE_STEP)


@pytest.fixture
def resolve_runner(tmp_path: Path) -> Callable[..., str]:
    """Return a callable that runs the resolve-version script.

    Wraps the script in a harness that stubs `gh release view`, wires
    `GITHUB_OUTPUT` to a tempfile, sets `GITHUB_REPOSITORY` to
    `CALLER_REPO`, and returns the `version=…` line as the function's
    result. The stub answers per `--repo`: `latest_tag` for
    `s0undt3ch/ToolR`, `CALLER_TAG` for anything else. With `gh_fails`
    it exports `GH_FAILS`, and the stub fails the way `gh` does.
    """
    script_body = _resolve_step_script()

    def run(
        *,
        input_version: str = "",
        latest_tag: str = "v0.20.1",
        gh_fails: bool = False,
    ) -> str:
        stub = f"""
gh() {{
  if [ "${{1:-}}" = "release" ] && [ "${{2:-}}" = "view" ]; then
    if [ -n "${{GH_FAILS:-}}" ]; then
      echo 'gh: release not found' >&2
      return 1
    fi
    local repo=""
    while [ "$#" -gt 0 ]; do
      if [ "$1" = "--repo" ]; then
        repo="${{2:-}}"
      fi
      shift
    done
    if [ "$repo" = "s0undt3ch/ToolR" ]; then
      printf '{latest_tag}'
    else
      printf '{CALLER_TAG}'
    fi
    return 0
  fi
}}
export -f gh
"""
        github_output = tmp_path / "GITHUB_OUTPUT"
        github_output.write_text("")
        wrapped = (
            "#!/bin/bash\n"
            "set -euo pipefail\n"
            f'INPUT_VERSION="{input_version}"\n'
            f'GITHUB_OUTPUT="{github_output}"\n'
            f'GITHUB_REPOSITORY="{CALLER_REPO}"\n'
            f'GH_FAILS="{"1" if gh_fails else ""}"\n'
            "export INPUT_VERSION GITHUB_OUTPUT GITHUB_REPOSITORY GH_FAILS\n" + stub + script_body
        )
        script_path = tmp_path / "wrapped.sh"
        script_path.write_text(wrapped)
        bash = shutil.which("bash")
        assert bash is not None, "bash disappeared between fixture setup and test body"
        result = subprocess.run(  # noqa: S603  # bash + script path are test-controlled
            [bash, str(script_path)],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode != 0:
            msg = (
                f"resolve-version script failed (rc={result.returncode}):\n"
                f"stderr:\n{result.stderr}\nstdout:\n{result.stdout}"
            )
            raise AssertionError(msg)
        output_lines = github_output.read_text().splitlines()
        for line in output_lines:
            if line.startswith("version="):
                return line.removeprefix("version=")
        msg = (
            f"no version=… in GITHUB_OUTPUT:\n{github_output.read_text()!r}\n"
            f"script stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
        raise AssertionError(msg)

    return run


@pytest.fixture(autouse=True)
def _skip_if_no_bash() -> None:
    if shutil.which("bash") is None:
        pytest.skip("bash not available")


@pytest.mark.parametrize(
    ("input_version", "expected"),
    [
        # Explicit `version:` with leading v.
        ("v0.20.1", "0.20.1"),
        # Explicit bare semver.
        ("0.20.1", "0.20.1"),
        # The literal `latest` — resolves via stubbed `gh release view`
        # against s0undt3ch/ToolR, not `CALLER_REPO`, whose own newest
        # tag (`CALLER_TAG`) would resolve to 9.9.9.
        ("latest", "0.20.1"),
        # Empty input — falls through to `latest`. Note: in practice
        # action.yml's `inputs.version.default` is non-empty post-bake-in,
        # so this branch only fires for explicit empty-string inputs.
        ("", "0.20.1"),
    ],
)
def test_resolve_returns_expected_version(
    resolve_runner: Callable[..., str],
    input_version: str,
    expected: str,
) -> None:
    assert resolve_runner(input_version=input_version) == expected


def test_resolve_below_minimum_version_fails(
    resolve_runner: Callable[..., str],
) -> None:
    """Pre-0.20.0 versions must produce a hard error.

    The minimum-version check is part of the resolve step — those
    versions shipped via pipx + PyPI source and aren't compatible
    with this action's binary-fetching shape.
    """
    with pytest.raises(AssertionError, match="minimum supported version"):
        resolve_runner(input_version="0.19.9")


def test_action_yml_inputs_version_default_is_set() -> None:
    """The bake-in mechanism (#281) requires a non-empty default.

    `toolr version bump` rewrites this during release prep. An empty
    default would mean SHA pins without `version:` input fall back to
    `latest`, defeating the bake-in's purpose.
    """
    data = yaml.safe_load(ACTION_YML.read_text())
    default = data["inputs"]["version"]["default"]
    assert default, (
        "action.yml inputs.version.default is empty — bake-in regressed; "
        "release prep should keep this in sync with the current release"
    )
    # Sanity: looks like a semver. Split into two asserts so ruff's
    # PT018 stays happy and so failure messages tell you which check
    # actually missed.
    parts = default.lstrip("v").split(".")
    assert len(parts) == 3, (
        f"action.yml inputs.version.default is not a 3-part version: {default!r}"
    )
    assert all(p.isdigit() for p in parts), (
        f"action.yml inputs.version.default has non-numeric parts: {default!r}"
    )


def test_latest_gh_failure_fails(resolve_runner: Callable[..., str]) -> None:
    """A failing `gh` aborts the step and its stderr reaches the log."""
    with pytest.raises(AssertionError, match="release not found"):
        resolve_runner(input_version="latest", gh_fails=True)
