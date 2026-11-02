"""Tests for `installation/install.sh`'s attestation check.

The tests cut `verify_attestation()` and its message helpers out of the
installer and run them under `sh` with a stub `gh` on a minimal `PATH`.

The stub stands in for `gh attestation verify --format json`, like the
one in `tests/action_yml/test_verify_attestation.py`: it enforces
`--cert-identity` the way `gh` does, and runs the installer's own `--jq`
expression with the real `jq` over the attestations that passed. That
keeps the decision (which top-level workflow built the archive, for
which file) testable offline.
"""

from __future__ import annotations

import hashlib
import json
import re
import shlex
import shutil
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]
INSTALL_SH = REPO_ROOT / "installation" / "install.sh"

WORKFLOWS = "https://github.com/s0undt3ch/ToolR/.github/workflows"
RELEASE_BUILD = f"{WORKFLOWS}/release.yml@refs/heads/main"
CI_BUILD = f"{WORKFLOWS}/ci.yml@refs/heads/main"
MAIN_SIGNER = f"{WORKFLOWS}/_build-binary-archive.yml@refs/heads/main"
ARCHIVE_NAME = "toolr-0.35.1-x86_64-unknown-linux-musl.tar.gz"
# The fixtures write an empty archive.
ARCHIVE_SHA256 = hashlib.sha256(b"").hexdigest()


def _subject(name: str = ARCHIVE_NAME, sha256: str = ARCHIVE_SHA256) -> dict[str, Any]:
    """An in-toto statement subject."""
    return {"name": name, "digest": {"sha256": sha256}}


REJECTED = "none from the s0undt3ch/ToolR release workflow on main for this exact archive"
SKIP_HINT = "--verify-attestation=skip"
GH_FLOOR = "install.sh: gh >= 2.56 is required to verify attestations"

pytestmark = [
    pytest.mark.skipif(sys.platform == "win32", reason="install.sh is POSIX-only"),
    pytest.mark.skipif(
        shutil.which("sh") is None or shutil.which("jq") is None,
        reason="sh and jq are required",
    ),
]

RunVerify = Callable[..., subprocess.CompletedProcess[str]]


def _verify_function_source() -> str:
    """`err`, `info`, `warn`, `sha256_of()` and `verify_attestation()` from install.sh."""
    text = INSTALL_SH.read_text()
    helpers = re.findall(r"^(?:err|info|warn)\(\) \{.*\}$", text, flags=re.MULTILINE)
    assert len(helpers) == 3, helpers
    functions = []
    for name in ("sha256_of", "verify_attestation"):
        body = re.search(rf"^{name}\(\) \{{\n.*?^\}}$", text, flags=re.MULTILINE | re.DOTALL)
        assert body is not None, f"no {name}() in install.sh"
        functions.append(body.group(0))
    return "\n".join([*helpers, *functions])


@pytest.fixture
def verify_runner(tmp_path: Path) -> RunVerify:
    """Return a callable that runs install.sh's `verify_attestation`.

    `build_configs` is the `buildConfigURI` of each attestation the stub
    `gh` finds. Each is signed by `signer`, which must equal the
    installer's `--cert-identity` as in real `gh`, and carries the
    statement `subjects` (default: one naming the archive under test, with
    its digest). `stdout` replaces the filtered output. A
    non-zero `gh_rc` makes the stub fail the way `gh attestation verify`
    does when nothing verifies. `gh_version` is what `gh --version`
    reports; `gh=False` leaves `gh` off `PATH`. `hashers` are the SHA-256
    tools on `PATH`: a name links the host's tool, a `(name, script)` pair
    installs a stub. Every other `gh` argument
    list is appended to `tmp_path / "gh_calls"`.
    """
    sh = shutil.which("sh")
    jq = shutil.which("jq")
    assert sh is not None
    assert jq is not None
    # Only the tools the function and the stub need, so a real `gh` on
    # the host's PATH can't leak in.
    tools = tmp_path / "tools"
    stub_bin = tmp_path / "stub-bin"
    stub_bin.mkdir()
    calls = tmp_path / "gh_calls"
    payload_file = tmp_path / "payload.json"
    stdout_file = tmp_path / "stdout.txt"
    archive = tmp_path / ARCHIVE_NAME
    archive.write_bytes(b"")
    script = _verify_function_source()

    def run(
        *,
        build_configs: list[str] | None = None,
        signer: str = MAIN_SIGNER,
        subjects: list[dict[str, Any]] | None = None,
        stdout: str | None = None,
        gh_rc: int = 0,
        gh_version: str = "2.103.2",
        gh: bool = True,
        mode: str = "require",
        repo: str = "s0undt3ch/ToolR",
        hashers: tuple[str | tuple[str, str], ...] = ("sha256sum", "shasum"),
    ) -> subprocess.CompletedProcess[str]:
        shutil.rmtree(tools, ignore_errors=True)
        tools.mkdir()
        for tool in ("sed", "head", "cat", "awk", *hashers):
            if isinstance(tool, tuple):
                name, body = tool
                (tools / name).write_text(f"#!{sh}\n{body}\n")
                (tools / name).chmod(0o755)
                continue
            found = shutil.which(tool)
            if found is not None:
                (tools / tool).symlink_to(found)
        payload_file.write_text(
            json.dumps(
                [
                    {
                        "verificationResult": {
                            "signature": {
                                "certificate": {
                                    "subjectAlternativeName": signer,
                                    "buildConfigURI": uri,
                                }
                            },
                            "statement": {
                                "subject": subjects if subjects is not None else [_subject()]
                            },
                        }
                    }
                    for uri in (build_configs or [])
                ]
            )
        )
        if stdout is None:
            emit = f'{shlex.quote(jq)} -c "$jq_expr" < {shlex.quote(str(payload_file))}'
        else:
            stdout_file.write_text(stdout + "\n")
            emit = f"cat {shlex.quote(str(stdout_file))}"
        stub = stub_bin / "gh"
        stub.unlink(missing_ok=True)
        if gh:
            stub.write_text(
                f"""#!{sh}
if [ "$1" = "--version" ]; then
  printf 'gh version %s (2026-01-01)\\nhttps://github.com/cli/cli/releases/tag/v%s\\n' \\
    {shlex.quote(gh_version)} {shlex.quote(gh_version)}
  exit 0
fi
printf '%s\\n' "$*" >> {shlex.quote(str(calls))}
if [ {gh_rc} -ne 0 ]; then
  echo 'gh: no matching attestations' >&2
  exit {gh_rc}
fi
jq_expr="." identity=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --jq) jq_expr="$2" ;;
    --cert-identity) identity="$2" ;;
  esac
  shift
done
if [ "$identity" != {shlex.quote(signer)} ]; then
  echo 'gh: no matching CertificateIdentity found' >&2
  exit 1
fi
{emit}
"""
            )
            stub.chmod(0o755)
        wrapped = (
            "set -eu\n"
            f"REPO={shlex.quote(repo)}\n"
            f"VERIFY_ATTESTATION={shlex.quote(mode)}\n"
            f"{script}\n"
            f"verify_attestation {shlex.quote(str(archive))}\n"
            "echo VERIFIED >&2\n"
        )
        script_path = tmp_path / "wrapped.sh"
        script_path.write_text(wrapped)
        return subprocess.run(  # noqa: S603  # sh + script path are test-controlled
            [sh, str(script_path)],
            capture_output=True,
            text=True,
            check=False,
            env={"PATH": f"{stub_bin}:{tools}", "HOME": str(tmp_path)},
        )

    return run


def test_rejects_ci_built_archive(verify_runner: RunVerify) -> None:
    """The reported shape: a valid in-repo attestation from a CI build."""
    result = verify_runner(build_configs=[CI_BUILD])
    assert result.returncode != 0
    assert REJECTED in result.stderr
    assert SKIP_HINT in result.stderr
    assert "VERIFIED" not in result.stderr


def test_accepts_release_built_archive(verify_runner: RunVerify) -> None:
    result = verify_runner(build_configs=[RELEASE_BUILD])
    assert result.returncode == 0, result.stderr
    assert "VERIFIED" in result.stderr


def test_accepts_when_any_attestation_is_release(verify_runner: RunVerify) -> None:
    result = verify_runner(build_configs=[CI_BUILD, RELEASE_BUILD])
    assert result.returncode == 0, result.stderr


@pytest.mark.parametrize(
    "build_config",
    [
        f"{WORKFLOWS}/release.yml-copy@refs/heads/main",
        "https://github.com/someone/ToolR/.github/workflows/release.yml@refs/heads/main",
        f"{WORKFLOWS}/release.yml@refs/heads/feature",
        f"{WORKFLOWS}/release.yml@refs/heads/main-evil",
        f"{WORKFLOWS}/release.yml@refs/pull/1/merge",
        f"{WORKFLOWS}/release.yml@refs/tags/v0.35.1",
    ],
)
def test_rejects_other_workflow_or_ref(verify_runner: RunVerify, build_config: str) -> None:
    result = verify_runner(build_configs=[build_config])
    assert result.returncode != 0
    assert REJECTED in result.stderr


def test_rejects_when_no_attestation_reported(verify_runner: RunVerify) -> None:
    result = verify_runner(build_configs=[])
    assert result.returncode != 0
    assert REJECTED in result.stderr


def test_gh_verify_failure_keeps_installer_message(verify_runner: RunVerify) -> None:
    result = verify_runner(build_configs=[RELEASE_BUILD], gh_rc=1)
    assert result.returncode != 0
    assert "no matching attestations" in result.stderr
    assert "install.sh: attestation verification failed for " in result.stderr


def test_passes_signer_and_hosted_runner_flags(verify_runner: RunVerify, tmp_path: Path) -> None:
    result = verify_runner(build_configs=[RELEASE_BUILD])
    assert result.returncode == 0, result.stderr
    call = (tmp_path / "gh_calls").read_text()
    assert call.startswith(f"attestation verify {tmp_path / ARCHIVE_NAME} ")
    assert "--repo s0undt3ch/ToolR " in call
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
def test_rejects_signer_off_main(verify_runner: RunVerify, signer: str) -> None:
    result = verify_runner(build_configs=[RELEASE_BUILD], signer=signer)
    assert result.returncode != 0
    assert "no matching CertificateIdentity" in result.stderr


@pytest.mark.parametrize("mode", ["require", "auto"])
@pytest.mark.parametrize("gh_version", ["2.55.0", "2.51.0", "1.99.0", "garbage"])
def test_rejects_gh_older_than_2_56(
    verify_runner: RunVerify, tmp_path: Path, gh_version: str, mode: str
) -> None:
    """An installed but outdated gh fails in `auto` too: it can't verify, so don't skip silently."""
    result = verify_runner(build_configs=[RELEASE_BUILD], gh_version=gh_version, mode=mode)
    assert result.returncode != 0
    assert GH_FLOOR in result.stderr
    assert SKIP_HINT in result.stderr
    assert not (tmp_path / "gh_calls").exists()


@pytest.mark.parametrize("gh_version", ["2.56.0", "2.103.2", "3.0.0"])
def test_accepts_gh_2_56_and_later(verify_runner: RunVerify, gh_version: str) -> None:
    result = verify_runner(build_configs=[RELEASE_BUILD], gh_version=gh_version)
    assert result.returncode == 0, result.stderr


@pytest.mark.parametrize(
    "subject",
    [
        "toolr-0.34.0-x86_64-unknown-linux-musl.tar.gz",
        "toolr-0.35.1-aarch64-unknown-linux-musl.tar.gz",
    ],
)
def test_rejects_attestation_for_another_archive(verify_runner: RunVerify, subject: str) -> None:
    """An older or other-triple release archive renamed into place is rejected."""
    result = verify_runner(build_configs=[RELEASE_BUILD], subjects=[_subject(subject)])
    assert result.returncode != 0
    assert REJECTED in result.stderr


@pytest.mark.parametrize("stdout", ["abc", "", "1 1"])
def test_non_numeric_count_fails_closed(verify_runner: RunVerify, stdout: str) -> None:
    result = verify_runner(build_configs=[RELEASE_BUILD], stdout=stdout)
    assert result.returncode != 0
    assert REJECTED in result.stderr


def test_skip_never_calls_gh(verify_runner: RunVerify, tmp_path: Path) -> None:
    result = verify_runner(build_configs=[CI_BUILD], mode="skip")
    assert result.returncode == 0, result.stderr
    assert "skipping attestation verification" in result.stderr
    assert not (tmp_path / "gh_calls").exists()


def test_require_without_gh_fails_closed(verify_runner: RunVerify) -> None:
    result = verify_runner(gh=False, mode="require")
    assert result.returncode != 0
    assert "the 'gh' CLI is not on PATH" in result.stderr
    assert SKIP_HINT in result.stderr


def test_auto_without_gh_warns_and_continues(verify_runner: RunVerify) -> None:
    result = verify_runner(gh=False, mode="auto")
    assert result.returncode == 0, result.stderr
    assert "NOT supply-chain verified" in result.stderr


def test_invalid_mode_fails(verify_runner: RunVerify) -> None:
    result = verify_runner(build_configs=[RELEASE_BUILD], mode="maybe")
    assert result.returncode != 0
    assert "invalid --verify-attestation value" in result.stderr


def test_repo_override_drives_the_identities(verify_runner: RunVerify, tmp_path: Path) -> None:
    """`TOOLR_REPO` points the whole check at another repository's release workflow."""
    fork = "https://github.com/someone/ToolR/.github/workflows"
    result = verify_runner(
        build_configs=[f"{fork}/release.yml@refs/heads/main"],
        signer=f"{fork}/_build-binary-archive.yml@refs/heads/main",
        repo="someone/ToolR",
    )
    assert result.returncode == 0, result.stderr
    call = (tmp_path / "gh_calls").read_text()
    assert "--repo someone/ToolR " in call
    # The fork's archives don't pass as ToolR releases, or vice versa.
    assert verify_runner(build_configs=[RELEASE_BUILD], repo="someone/ToolR").returncode != 0
    result = verify_runner(
        build_configs=[RELEASE_BUILD],
        signer=f"{fork}/_build-binary-archive.yml@refs/heads/main",
        repo="someone/ToolR",
    )
    assert result.returncode != 0
    assert "none from the someone/ToolR release workflow on main" in result.stderr


@pytest.mark.skipif(shutil.which("openssl") is None, reason="openssl is required")
def test_digest_falls_back_to_openssl(verify_runner: RunVerify) -> None:
    result = verify_runner(build_configs=[RELEASE_BUILD], hashers=("openssl",))
    assert result.returncode == 0, result.stderr


def test_no_sha256_tool_fails_closed(verify_runner: RunVerify, tmp_path: Path) -> None:
    result = verify_runner(build_configs=[RELEASE_BUILD], hashers=())
    assert result.returncode != 0
    assert "no sha256 tool available" in result.stderr
    assert not (tmp_path / "gh_calls").exists()


@pytest.mark.parametrize(
    "hasher",
    [
        pytest.param(("sha256sum", "echo 'sha256sum: read error' >&2; exit 1"), id="fails"),
        pytest.param(("sha256sum", "echo 'not-a-digest  -'"), id="garbage"),
        pytest.param(("sha256sum", f"echo '{ARCHIVE_SHA256.upper()}  -'"), id="uppercase"),
        pytest.param(("sha256sum", f"echo '{ARCHIVE_SHA256[:-1]}  -'"), id="short"),
        pytest.param(("openssl", "echo 'unexpected output'"), id="openssl-garbage"),
    ],
)
def test_unusable_digest_fails_closed(
    verify_runner: RunVerify, tmp_path: Path, hasher: tuple[str, str]
) -> None:
    """No pipefail in sh, so a broken hasher must not pass an empty or bogus digest on."""
    result = verify_runner(build_configs=[RELEASE_BUILD], hashers=(hasher,))
    assert result.returncode != 0
    assert "install.sh: could not compute the SHA-256 of " in result.stderr
    assert SKIP_HINT in result.stderr
    assert not (tmp_path / "gh_calls").exists()


def test_rejects_subject_with_another_digest(verify_runner: RunVerify) -> None:
    """The name matches but the digest is another file's."""
    result = verify_runner(build_configs=[RELEASE_BUILD], subjects=[_subject(sha256="0" * 64)])
    assert result.returncode != 0
    assert REJECTED in result.stderr


def test_rejects_name_and_digest_on_different_subjects(verify_runner: RunVerify) -> None:
    """One subject must carry both: a name from one and a digest from another doesn't count."""
    result = verify_runner(
        build_configs=[RELEASE_BUILD],
        subjects=[
            _subject(sha256="0" * 64),
            _subject("toolr-0.34.0-x86_64-unknown-linux-musl.tar.gz"),
        ],
    )
    assert result.returncode != 0
    assert REJECTED in result.stderr
