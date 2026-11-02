"""Tests for `installation/install.sh`'s attestation check.

The tests cut `verify_attestation()` and its message helpers out of the
installer and run them under `sh` with a stub `gh` on a minimal `PATH`.

The stub comes from `tests/attestation_stub.py`, shared with the
action's tests.
"""

from __future__ import annotations

import re
import shlex
import shutil
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest

from tests.attestation_stub import ARCHIVE_NAME
from tests.attestation_stub import ARCHIVE_SHA256
from tests.attestation_stub import CI_BUILD
from tests.attestation_stub import MAIN_SIGNER
from tests.attestation_stub import RELEASE_BUILD
from tests.attestation_stub import WORKFLOWS
from tests.attestation_stub import subject
from tests.attestation_stub import write_gh_stub

REPO_ROOT = Path(__file__).resolve().parents[2]
INSTALL_SH = REPO_ROOT / "installation" / "install.sh"

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

    `gh=False` leaves `gh` off `PATH`; any other keyword argument goes to
    `write_gh_stub`. `mode` and `repo` set `VERIFY_ATTESTATION` and
    `REPO`. `hashers` are the SHA-256 tools on `PATH`: a name links the
    host's tool, a `(name, script)` pair installs a stub. Every `gh`
    argument list but `--version` is appended to `tmp_path / "gh_calls"`.
    """
    sh = shutil.which("sh")
    assert sh is not None
    # Only the tools the function needs, so a real `gh` on the host's
    # PATH can't leak in.
    tools = tmp_path / "tools"
    stub_bin = tmp_path / "stub-bin"
    stub_bin.mkdir()
    archive = tmp_path / ARCHIVE_NAME
    archive.write_bytes(b"")
    script = _verify_function_source()

    def run(
        *,
        gh: bool = True,
        mode: str = "require",
        repo: str = "s0undt3ch/ToolR",
        hashers: tuple[str | tuple[str, str], ...] = ("sha256sum", "shasum"),
        **stub: Any,
    ) -> subprocess.CompletedProcess[str]:
        shutil.rmtree(tools, ignore_errors=True)
        tools.mkdir()
        for tool in ("sed", "head", "awk", *hashers):
            if isinstance(tool, tuple):
                name, body = tool
                (tools / name).write_text(f"#!{sh}\n{body}\n")
                (tools / name).chmod(0o755)
                continue
            found = shutil.which(tool)
            if found is not None:
                (tools / tool).symlink_to(found)
        if gh:
            write_gh_stub(stub_bin, tmp_path / "gh_calls", **{"build_configs": [], **stub})
        else:
            (stub_bin / "gh").unlink(missing_ok=True)
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
    "other_archive",
    [
        "toolr-0.34.0-x86_64-unknown-linux-musl.tar.gz",
        "toolr-0.35.1-aarch64-unknown-linux-musl.tar.gz",
    ],
)
def test_rejects_attestation_for_another_archive(
    verify_runner: RunVerify, other_archive: str
) -> None:
    """An older or other-triple release archive renamed into place is rejected."""
    result = verify_runner(build_configs=[RELEASE_BUILD], subjects=[subject(other_archive)])
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
    result = verify_runner(build_configs=[RELEASE_BUILD], subjects=[subject(sha256="0" * 64)])
    assert result.returncode != 0
    assert REJECTED in result.stderr


def test_rejects_name_and_digest_on_different_subjects(verify_runner: RunVerify) -> None:
    """One subject must carry both: a name from one and a digest from another doesn't count."""
    result = verify_runner(
        build_configs=[RELEASE_BUILD],
        subjects=[
            subject(sha256="0" * 64),
            subject("toolr-0.34.0-x86_64-unknown-linux-musl.tar.gz"),
        ],
    )
    assert result.returncode != 0
    assert REJECTED in result.stderr
