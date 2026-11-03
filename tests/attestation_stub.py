"""A stub `gh` for tests of ToolR's attestation checks.

Shared by `tests/action_yml/test_verify_attestation.py` (the action's
step) and `tests/installation/test_install_sh_attestation.py` (the
installer). The stub stands in for `gh attestation verify --format json`:
it enforces `--cert-identity` the way `gh` does, and runs the caller's own
`--jq` expression with the real `jq` over the attestations that passed.
That keeps the decision (which top-level workflow built the archive, for
which file) testable offline.

The identity literals here are the release pins. Moving releases off
`main` changes them here, in the scripts, and in the docs that
`tests/installation/test_attestation_pins.py` keeps in step.
"""

from __future__ import annotations

import hashlib
import json
import shlex
from pathlib import Path
from typing import Any

WORKFLOWS = "https://github.com/s0undt3ch/ToolR/.github/workflows"
RELEASE_BUILD = f"{WORKFLOWS}/release.yml@refs/heads/main"
CI_BUILD = f"{WORKFLOWS}/ci.yml@refs/heads/main"
MAIN_SIGNER = f"{WORKFLOWS}/_build-binary-archive.yml@refs/heads/main"
ARCHIVE_NAME = "toolr-0.35.1-x86_64-unknown-linux-musl.tar.gz"
# Tests write an empty archive.
ARCHIVE_SHA256 = hashlib.sha256(b"").hexdigest()


def subject(name: str = ARCHIVE_NAME, sha256: str = ARCHIVE_SHA256) -> dict[str, Any]:
    """An in-toto statement subject."""
    return {"name": name, "digest": {"sha256": sha256}}


def write_gh_stub(
    bin_dir: Path,
    calls: Path,
    *,
    build_configs: list[str],
    signer: str = MAIN_SIGNER,
    subjects: list[dict[str, Any]] | None = None,
    stdout: str | None = None,
    gh_rc: int = 0,
    gh_version: str = "2.103.2",
) -> Path:
    """Write an executable `gh` into `bin_dir` and return its path.

    `build_configs` is the `buildConfigURI` of each attestation the stub
    finds. Each is signed by `signer`, which must equal the caller's
    `--cert-identity` as in real `gh`, and carries the statement
    `subjects` (default: one naming `ARCHIVE_NAME` with its digest).
    `stdout` replaces the filtered output. A non-zero `gh_rc` makes the
    stub fail the way `gh attestation verify` does when nothing
    verifies. `gh_version` is what `gh --version` reports. Every other
    argument list is appended to `calls`.

    The stub runs `bash` and `jq` from `PATH`. Paths go into it in POSIX
    form, with LF line endings, so Git Bash on Windows can read it: a
    native `C:\\...` path in a shebang or a CR before a newline breaks it.
    """
    bin_dir.mkdir(parents=True, exist_ok=True)
    payload = bin_dir / "gh-payload.json"
    payload.write_text(
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
                        "statement": {"subject": subjects if subjects is not None else [subject()]},
                    }
                }
                for uri in build_configs
            ]
        )
    )
    if stdout is None:
        emit = f'exec jq -c "$jq_expr" < {shlex.quote(payload.as_posix())}'
    else:
        emit = f"printf '%s\\n' {shlex.quote(stdout)}"
    stub = bin_dir / "gh"
    stub.write_text(
        f"""#!/usr/bin/env bash
if [ "$1" = "--version" ]; then
  printf 'gh version %s (2026-01-01)\\nhttps://github.com/cli/cli/releases/tag/v%s\\n' \\
    {shlex.quote(gh_version)} {shlex.quote(gh_version)}
  exit 0
fi
printf '%s\\n' "$*" >> {shlex.quote(calls.as_posix())}
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
""",
        newline="\n",
    )
    stub.chmod(0o755)
    return stub
