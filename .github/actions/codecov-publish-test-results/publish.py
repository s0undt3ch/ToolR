"""Upload staged Codecov test results, one upload per staged directory, then send notifications.

Each directory under the download root holds the JUnit files and a `meta.json`
written by the `codecov-stage-test-results` action. Standard library only: it runs
on the runner's system Python, outside any project environment.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import urllib.parse
import urllib.request
from collections.abc import Callable
from collections.abc import Sequence
from pathlib import Path
from typing import Any

OIDC_AUDIENCE = "https://codecov.io"


class PublishError(Exception):
    """A problem the workflow author has to fix."""


def pull_request(event_path: str | None) -> dict[str, Any]:
    """Return the `pull_request` object of the triggering event, or `{}` when there is none."""
    if not event_path:
        return {}
    event = json.loads(Path(event_path).read_text(encoding="utf-8"))
    pr = event.get("pull_request") if isinstance(event, dict) else None
    return pr if isinstance(pr, dict) else {}


def is_fork(pr: dict[str, Any], repository: str) -> bool:
    """Return whether the PR comes from another repository, which gets no OIDC token."""
    head_repo = (pr.get("head") or {}).get("repo") or {}
    full_name = head_repo.get("full_name")
    return bool(full_name) and full_name != repository


def commit_sha(pr: dict[str, Any], env: dict[str, str]) -> str:
    """Return the PR head commit, or `GITHUB_SHA` outside PRs.

    On PRs `GITHUB_SHA` is the merge commit, which Codecov doesn't know about.
    """
    sha = (pr.get("head") or {}).get("sha") or env.get("GITHUB_SHA")
    if not sha:
        msg = (
            "can't tell which commit to report against: no pull_request.head.sha and no GITHUB_SHA"
        )
        raise PublishError(msg)
    return str(sha)


def fetch_oidc_token(
    request_url: str,
    request_token: str,
    opener: Callable[..., Any] = urllib.request.urlopen,  # noqa: S310 - runner-provided https URL
) -> str:
    """Request a GitHub OIDC token for Codecov, as `core.getIDToken` does."""
    separator = "&" if urllib.parse.urlsplit(request_url).query else "?"
    url = f"{request_url}{separator}audience={urllib.parse.quote(OIDC_AUDIENCE, safe='')}"
    request = urllib.request.Request(url, headers={"Authorization": f"bearer {request_token}"})  # noqa: S310 - runner-provided https URL
    with opener(request, timeout=30) as response:
        value = json.load(response).get("value")
    if not value:
        msg = "the OIDC token response carried no token"
        raise PublishError(msg)
    return str(value)


def resolve_token(fork: bool, env: dict[str, str], opener: Callable[..., Any]) -> str | None:
    """Return the upload token: none for forks (Codecov accepts those tokenless), OIDC otherwise."""
    if fork:
        return None
    request_url = env.get("ACTIONS_ID_TOKEN_REQUEST_URL")
    request_token = env.get("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
    if not request_url or not request_token:
        msg = "no OIDC token available; give the job `permissions: id-token: write`"
        raise PublishError(msg)
    return fetch_oidc_token(request_url, request_token, opener)


def staged_dirs(root: Path) -> list[Path]:
    """Return the staged directories under `root`, sorted for a stable upload order."""
    if not root.is_dir():
        return []
    return sorted(path for path in root.iterdir() if path.is_dir())


def read_meta(directory: Path) -> tuple[list[Path], list[str], str]:
    """Return a staged directory's files, flags and name from its `meta.json`."""
    meta_path = directory / "meta.json"
    try:
        meta = json.loads(meta_path.read_text(encoding="utf-8-sig"))
    except (OSError, json.JSONDecodeError) as exc:
        msg = f"{meta_path}: unreadable ({exc})"
        raise PublishError(msg) from exc
    files = meta.get("files") if isinstance(meta, dict) else None
    flags = meta.get("flags") if isinstance(meta, dict) else None
    name = meta.get("name") if isinstance(meta, dict) else None
    if not isinstance(files, list) or not files or not all(isinstance(f, str) for f in files):
        msg = f"{meta_path}: `files` must be a non-empty list of file names"
        raise PublishError(msg)
    if not isinstance(flags, list) or not all(isinstance(f, str) for f in flags):
        msg = f"{meta_path}: `flags` must be a list of strings"
        raise PublishError(msg)
    if not isinstance(name, str):
        msg = f"{meta_path}: `name` must be a string"
        raise PublishError(msg)
    paths = [directory / file for file in files]
    missing = [str(path) for path in paths if not path.is_file()]
    if missing:
        msg = f"{meta_path}: listed files are missing: {', '.join(missing)}"
        raise PublishError(msg)
    return paths, flags, name


def notify_args(sha: str) -> list[str]:
    """Arguments for `send-notifications`, which takes no `--pr` or `--branch`."""
    return ["--sha", sha, "--git-service", "github"]


def upload_args(sha: str, pr: dict[str, Any], fork: bool) -> list[str]:
    """Arguments for each upload; forks also name the PR and branch, as codecov-action does."""
    args = notify_args(sha)
    if fork:
        args += [
            "--pr",
            str(pr.get("number", "")),
            "--branch",
            str((pr.get("head") or {}).get("label", "")),
        ]
    return args


def upload_command(
    cli: str, files: Sequence[Path], flags: Sequence[str], name: str, common: Sequence[str]
) -> list[str]:
    """Build one `upload-process` call for a staged directory."""
    command = [cli, "upload-process", "--report-type", "test_results", "--disable-search", *common]
    for file in files:
        command += ["--file", str(file)]
    for flag in flags:
        command += ["--flag", flag]
    if name:
        command += ["--name", name]
    return command


def main(
    argv: Sequence[str] | None = None,
    env: dict[str, str] | None = None,
    opener: Callable[..., Any] = urllib.request.urlopen,  # noqa: S310 - runner-provided https URL
    run: Callable[..., Any] = subprocess.run,
) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "root", type=Path, help="directory the staged artifacts were downloaded into"
    )
    parser.add_argument("--cli", default="codecovcli", help="codecovcli executable")
    parser.add_argument(
        "--no-send-notifications", action="store_true", help="skip send-notifications"
    )
    args = parser.parse_args(argv)
    env = dict(os.environ) if env is None else env

    try:
        pr = pull_request(env.get("GITHUB_EVENT_PATH"))
        fork = is_fork(pr, env.get("GITHUB_REPOSITORY", ""))
        sha = commit_sha(pr, env)
        token = resolve_token(fork, env, opener)
        commands = [
            upload_command(args.cli, *read_meta(d), upload_args(sha, pr, fork))
            for d in staged_dirs(args.root)
        ]
    except PublishError as exc:
        print(f"::error::{exc}", flush=True)
        return 1

    if not commands:
        print(
            f"::warning::No staged Codecov test results under {args.root}; nothing to upload.",
            flush=True,
        )
    if not args.no_send_notifications:
        commands.append([args.cli, "send-notifications", *notify_args(sha)])

    child_env = dict(env)
    child_env.pop("CODECOV_TOKEN", None)
    if token:
        # The CLI reads `--token` from `CODECOV_TOKEN` (its `TOKEN` envvar, brand-prefixed);
        # keeping it off argv keeps it out of process listings.
        child_env["CODECOV_TOKEN"] = token
    # Keep going after a failed upload: skipping send-notifications would hold back
    # the coverage results too.
    failed = 0
    for command in commands:
        print(f"::group::{' '.join(command[1:])}", flush=True)
        returncode = run(command, env=child_env, check=False).returncode
        print("::endgroup::", flush=True)
        if returncode:
            print(f"::error::`{' '.join(command[1:])}` exited with {returncode}", flush=True)
            failed += 1
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
