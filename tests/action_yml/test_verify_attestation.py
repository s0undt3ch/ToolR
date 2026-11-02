"""Tests for the `setup-toolr` composite action's attestation step.

The "Verify SLSA build provenance" step is a bash script embedded in
`action.yml`. These tests extract its body, prepend a stubbed `gh` shell
function and run the result through `bash`.

The stub stands in for `gh attestation verify --format json`: it enforces
`--cert-identity` the way `gh` does, prints the attestations that passed,
each carrying the `buildConfigURI` of the workflow run that produced it,
and applies the step's own `--jq` expression with the real `jq`. That
keeps the decision logic (which top-level workflow built the archive)
testable offline. The step's literal `env:` values (the pinned workflow
identities) are exported first, as the runner would.
"""

from __future__ import annotations

import json
import shlex
import shutil
import subprocess
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest
import yaml

REPO_ROOT = Path(__file__).resolve().parents[2]
ACTION_YML = REPO_ROOT / "action.yml"

WORKFLOWS = "https://github.com/s0undt3ch/ToolR/.github/workflows"
RELEASE_BUILD = f"{WORKFLOWS}/release.yml@refs/heads/main"
CI_BUILD = f"{WORKFLOWS}/ci.yml@refs/heads/main"
MAIN_SIGNER = f"{WORKFLOWS}/_build-binary-archive.yml@refs/heads/main"
ARCHIVE_NAME = "toolr-0.35.1-x86_64-unknown-linux-musl.tar.gz"
REJECTED = "not built by the ToolR release workflow on main"


def _step(step_id: str) -> dict[str, Any]:
    """Return an action.yml step by its `id`."""
    data = yaml.safe_load(ACTION_YML.read_text())
    for step in data["runs"]["steps"]:
        if step.get("id") == step_id:
            return step
    msg = f"no `{step_id}` step in action.yml"
    raise AssertionError(msg)


def _static_env(step: dict[str, Any]) -> dict[str, str]:
    """The step's `env:` entries that hold no `${{ }}` expression."""
    return {k: v for k, v in step.get("env", {}).items() if "${{" not in v}


@pytest.fixture(autouse=True)
def _skip_if_no_bash_or_jq() -> None:
    if shutil.which("bash") is None or shutil.which("jq") is None:
        pytest.skip("bash and jq are required")


@pytest.fixture
def verify_runner(tmp_path: Path) -> Callable[..., subprocess.CompletedProcess[str]]:
    """Return a callable that runs the verify-attestation script.

    `build_configs` is the `buildConfigURI` of each attestation the
    stubbed `gh` finds. Each is signed by `signer`, which must equal the
    step's `--cert-identity` as in real `gh`, and names `subject`
    (default: the archive under test) as its statement subject.
    `stdout` replaces the filtered output. A
    non-zero `gh_rc` makes the stub fail the way `gh attestation verify`
    does when no attestation verifies. `gh_version` is what
    `gh --version` reports. Every other `gh` argument list is appended to
    `tmp_path / "gh_calls"`.
    """
    step = _step("verify-attestation")
    exports = "".join(
        f"{name}={shlex.quote(value)}\nexport {name}\n" for name, value in _static_env(step).items()
    )
    script_body = step["run"]
    calls = tmp_path / "gh_calls"

    def run(
        *,
        build_configs: list[str],
        signer: str = MAIN_SIGNER,
        subject: str = ARCHIVE_NAME,
        stdout: str | None = None,
        gh_rc: int = 0,
        gh_version: str = "2.103.2",
    ) -> subprocess.CompletedProcess[str]:
        payload = json.dumps(
            [
                {
                    "verificationResult": {
                        "signature": {
                            "certificate": {
                                "subjectAlternativeName": signer,
                                "buildConfigURI": uri,
                            }
                        },
                        "statement": {"subject": [{"name": subject}]},
                    }
                }
                for uri in build_configs
            ]
        )
        if stdout is None:
            emit = f'printf "%s" {shlex.quote(payload)} | jq -c "$jq_expr"'
        else:
            emit = f"printf '%s\\n' {shlex.quote(stdout)}"
        stub = f"""
gh() {{
  if [ "$1" = "--version" ]; then
    printf 'gh version %s (2026-01-01)\\nhttps://github.com/cli/cli/releases/tag/v%s\\n' \\
      {shlex.quote(gh_version)} {shlex.quote(gh_version)}
    return 0
  fi
  printf '%s\\n' "$*" >> {shlex.quote(str(calls))}
  if [ {gh_rc} -ne 0 ]; then
    echo 'gh: no matching attestations' >&2
    return {gh_rc}
  fi
  local jq_expr="." identity=""
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --jq) jq_expr="$2" ;;
      --cert-identity) identity="$2" ;;
    esac
    shift
  done
  if [ "$identity" != {shlex.quote(signer)} ]; then
    echo 'gh: no matching CertificateIdentity found' >&2
    return 1
  fi
  {emit}
}}
"""
        archive = tmp_path / ARCHIVE_NAME
        archive.write_bytes(b"")
        wrapped = (
            "#!/bin/bash\n"
            "set -euo pipefail\n"
            f"ARCHIVE_PATH={shlex.quote(str(archive))}\n"
            "export ARCHIVE_PATH\n" + exports + stub + script_body
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
        f"{WORKFLOWS}/release.yml@refs/heads/feature",
        f"{WORKFLOWS}/release.yml@refs/heads/main-evil",
        f"{WORKFLOWS}/release.yml@refs/pull/1/merge",
        f"{WORKFLOWS}/release.yml@refs/tags/v0.35.1",
    ],
)
def test_rejects_other_workflow_or_ref(
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
    assert f"--cert-identity {MAIN_SIGNER} " in call
    assert "--signer-workflow" not in call
    assert "--deny-self-hosted-runners" in call
    assert "--format json" in call


@pytest.mark.parametrize(
    "signer",
    [
        f"{WORKFLOWS}/_build-binary-archive.yml@refs/heads/feature",
        f"{WORKFLOWS}/_build-binary-archive.yml@refs/heads/main-evil",
    ],
)
def test_rejects_signer_off_main(
    verify_runner: Callable[..., subprocess.CompletedProcess[str]],
    signer: str,
) -> None:
    """`--signer-workflow` would match `main-evil` by prefix; `--cert-identity` is exact."""
    result = verify_runner(build_configs=[RELEASE_BUILD], signer=signer)
    assert result.returncode != 0
    assert "no matching CertificateIdentity" in result.stderr


@pytest.mark.parametrize("gh_version", ["2.55.0", "2.51.0", "1.99.0", "garbage"])
def test_rejects_gh_older_than_2_56(
    verify_runner: Callable[..., subprocess.CompletedProcess[str]],
    tmp_path: Path,
    gh_version: str,
) -> None:
    """gh before 2.56 can't verify ToolR attestations, so fail before calling it."""
    result = verify_runner(build_configs=[RELEASE_BUILD], gh_version=gh_version)
    assert result.returncode != 0
    assert "setup-toolr: gh >= 2.56 is required to verify attestations" in result.stderr
    assert not (tmp_path / "gh_calls").exists()


@pytest.mark.parametrize("gh_version", ["2.56.0", "2.103.2", "3.0.0"])
def test_accepts_gh_2_56_and_later(
    verify_runner: Callable[..., subprocess.CompletedProcess[str]],
    gh_version: str,
) -> None:
    result = verify_runner(build_configs=[RELEASE_BUILD], gh_version=gh_version)
    assert result.returncode == 0, result.stderr


@pytest.mark.parametrize(
    "subject",
    [
        "toolr-0.34.0-x86_64-unknown-linux-musl.tar.gz",
        "toolr-0.35.1-aarch64-unknown-linux-musl.tar.gz",
    ],
)
def test_rejects_attestation_for_another_archive(
    verify_runner: Callable[..., subprocess.CompletedProcess[str]],
    subject: str,
) -> None:
    """An older or other-triple release archive renamed into place is rejected."""
    result = verify_runner(build_configs=[RELEASE_BUILD], subject=subject)
    assert result.returncode != 0
    assert REJECTED in result.stderr


@pytest.mark.parametrize("stdout", ["abc", "", "1 1"])
def test_non_numeric_count_fails_closed(
    verify_runner: Callable[..., subprocess.CompletedProcess[str]],
    stdout: str,
) -> None:
    result = verify_runner(build_configs=[RELEASE_BUILD], stdout=stdout)
    assert result.returncode != 0
    assert REJECTED in result.stderr


def test_pinned_workflows_match_the_release_pipeline() -> None:
    """The pinned identities name real workflows wired the way releases run.

    Renaming either workflow, or `release.yml` no longer calling the
    archive builder, would make every release fail the action's check.
    """
    pins = _static_env(_step("verify-attestation"))
    prefix = "https://github.com/s0undt3ch/ToolR/"
    paths = {}
    for name in ("SIGNER_WORKFLOW", "RELEASE_WORKFLOW"):
        path, _, ref = pins[name].removeprefix(prefix).partition("@")
        assert pins[name].startswith(prefix), pins[name]
        assert ref == "refs/heads/main", pins[name]
        assert (REPO_ROOT / path).is_file(), path
        paths[name] = path
    assert pins["SIGNER_WORKFLOW"] == MAIN_SIGNER
    assert pins["RELEASE_WORKFLOW"] == RELEASE_BUILD

    release = yaml.safe_load((REPO_ROOT / paths["RELEASE_WORKFLOW"]).read_text())
    callers = [job.get("uses") for job in release["jobs"].values()]
    assert f"./{paths['SIGNER_WORKFLOW']}" in callers
    # publish-packslip signs as the same release workflow identity.
    assert (
        f"--identity {pins['RELEASE_WORKFLOW']} "
        in (REPO_ROOT / paths["RELEASE_WORKFLOW"]).read_text()
    )
