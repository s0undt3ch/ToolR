"""Tests for the `setup-toolr` composite action's attestation step.

The "Verify SLSA build provenance" step is a bash script embedded in
`action.yml`. These tests extract its body, prepend a stubbed `gh` shell
function and run the result through `bash`.

The stub stands in for `gh attestation verify --format json`: it prints
attestations that already passed `gh`'s checks, each carrying the
`buildConfigURI` of the workflow run that produced it, and applies the
step's own `--jq` expression with the real `jq`. That keeps the decision
logic (which top-level workflow built the archive) testable offline.
"""

from __future__ import annotations

import json
import shlex
import shutil
import subprocess
from collections.abc import Callable
from pathlib import Path

import pytest
import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
ACTION_YML = REPO_ROOT / "action.yml"

WORKFLOWS = "https://github.com/s0undt3ch/ToolR/.github/workflows"
RELEASE_BUILD = f"{WORKFLOWS}/release.yml@refs/heads/main"
CI_BUILD = f"{WORKFLOWS}/ci.yml@refs/heads/main"
REJECTED = "not built by the ToolR release workflow"


def _step_script(step_id: str) -> str:
    """Pull a step's `run:` body out of action.yml by its `id`."""
    data = yaml.safe_load(ACTION_YML.read_text())
    for step in data["runs"]["steps"]:
        if step.get("id") == step_id:
            return step["run"]
    msg = f"no `{step_id}` step in action.yml"
    raise AssertionError(msg)


@pytest.fixture(autouse=True)
def _skip_if_no_bash_or_jq() -> None:
    if shutil.which("bash") is None or shutil.which("jq") is None:
        pytest.skip("bash and jq are required")


@pytest.fixture
def verify_runner(tmp_path: Path) -> Callable[..., subprocess.CompletedProcess[str]]:
    """Return a callable that runs the verify-attestation script.

    `build_configs` is the `buildConfigURI` of each verified attestation
    the stubbed `gh` reports. A non-zero `gh_rc` makes the stub fail the
    way `gh attestation verify` does when no attestation verifies. Every
    `gh` argument list is appended to `tmp_path / "gh_calls"`.
    """
    script_body = _step_script("verify-attestation")
    calls = tmp_path / "gh_calls"

    def run(*, build_configs: list[str], gh_rc: int = 0) -> subprocess.CompletedProcess[str]:
        payload = json.dumps(
            [
                {"verificationResult": {"signature": {"certificate": {"buildConfigURI": uri}}}}
                for uri in build_configs
            ]
        )
        stub = f"""
gh() {{
  printf '%s\\n' "$*" >> {shlex.quote(str(calls))}
  if [ {gh_rc} -ne 0 ]; then
    echo 'gh: no matching attestations' >&2
    return {gh_rc}
  fi
  local jq_expr="."
  while [ "$#" -gt 0 ]; do
    if [ "$1" = "--jq" ]; then
      jq_expr="$2"
    fi
    shift
  done
  printf '%s' {shlex.quote(payload)} | jq -c "$jq_expr"
}}
"""
        archive = tmp_path / "toolr-0.35.1-x86_64-unknown-linux-musl.tar.gz"
        archive.write_bytes(b"")
        wrapped = (
            "#!/bin/bash\n"
            "set -euo pipefail\n"
            f"ARCHIVE_PATH={shlex.quote(str(archive))}\n"
            "export ARCHIVE_PATH\n" + stub + script_body
        )
        script_path = tmp_path / "wrapped.sh"
        script_path.write_text(wrapped)
        bash = shutil.which("bash")
        assert bash is not None, "bash disappeared between fixture setup and test body"
        return subprocess.run(  # noqa: S603  # bash + script path are test-controlled
            [bash, str(script_path)],
            capture_output=True,
            text=True,
            check=False,
        )

    return run


def test_rejects_ci_built_archive(
    verify_runner: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    """The reported shape: a valid in-repo attestation from a CI build."""
    result = verify_runner(build_configs=[CI_BUILD])
    assert result.returncode != 0
    assert REJECTED in result.stderr
    assert "skip-attestation" in result.stderr


def test_accepts_release_built_archive(
    verify_runner: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    result = verify_runner(build_configs=[RELEASE_BUILD])
    assert result.returncode == 0, result.stderr


def test_accepts_when_any_attestation_is_release(
    verify_runner: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    result = verify_runner(build_configs=[CI_BUILD, RELEASE_BUILD])
    assert result.returncode == 0, result.stderr


@pytest.mark.parametrize(
    "build_config",
    [
        f"{WORKFLOWS}/release.yml-copy@refs/heads/main",
        f"{WORKFLOWS}/release.yml.evil@refs/heads/main",
        "https://github.com/someone/ToolR/.github/workflows/release.yml@refs/heads/main",
    ],
)
def test_rejects_lookalike_workflow(
    verify_runner: Callable[..., subprocess.CompletedProcess[str]],
    build_config: str,
) -> None:
    result = verify_runner(build_configs=[build_config])
    assert result.returncode != 0
    assert REJECTED in result.stderr


def test_rejects_when_no_attestation_reported(
    verify_runner: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    result = verify_runner(build_configs=[])
    assert result.returncode != 0
    assert REJECTED in result.stderr


def test_gh_verify_failure_fails(
    verify_runner: Callable[..., subprocess.CompletedProcess[str]],
) -> None:
    result = verify_runner(build_configs=[RELEASE_BUILD], gh_rc=1)
    assert result.returncode != 0
    assert "no matching attestations" in result.stderr


def test_passes_signer_and_hosted_runner_flags(
    verify_runner: Callable[..., subprocess.CompletedProcess[str]],
    tmp_path: Path,
) -> None:
    result = verify_runner(build_configs=[RELEASE_BUILD])
    assert result.returncode == 0, result.stderr
    call = (tmp_path / "gh_calls").read_text()
    assert call.startswith("attestation verify ")
    assert "--repo s0undt3ch/ToolR" in call
    assert "--signer-workflow s0undt3ch/ToolR/.github/workflows/_build-binary-archive.yml" in call
    assert "--deny-self-hosted-runners" in call
    assert "--format json" in call
