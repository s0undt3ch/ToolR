"""Versioning utilities."""

from __future__ import annotations

import os
import tomllib
from datetime import UTC
from datetime import datetime
from pathlib import Path
from typing import Annotated
from typing import Final

from toolr import Context
from toolr import arg
from toolr import command_group

# Built-in fallback for when there are no tags (e.g. brand-new repo).
# Avoid GNU-only strftime extensions (e.g. `%-m`) — Windows' C runtime rejects
# them, which breaks module import (and pytest collection) on Windows.
_NOW: Final[datetime] = datetime.now(UTC)
TODAY_VERSION: Final[str] = f"{_NOW.year % 100}.{_NOW.month}.0"

CARGO_TOML_PATH: Final[Path] = Path("Cargo.toml")

group = command_group("version", "Versioning utilities", docstring=__doc__)


def _read_workspace_version(cargo_toml: Path = CARGO_TOML_PATH) -> str:
    """Return `[workspace.package] version` from the root Cargo.toml."""
    with cargo_toml.open("rb") as f:
        data = tomllib.load(f)
    try:
        return data["workspace"]["package"]["version"]
    except KeyError as exc:
        msg = f"No [workspace.package].version in {cargo_toml}"
        raise ValueError(msg) from exc


def _set_workspace_version(ctx: Context, new_version: str) -> None:
    """Update `[workspace.package] version` via ``cargo set-version``."""
    ret = ctx.run("cargo", "set-version", "--workspace", new_version)
    if ret.returncode != 0:
        ctx.error(f"cargo set-version failed with exit code {ret.returncode}")
        ctx.exit(ret.returncode)


def _compute_dev_version(ctx: Context) -> str:
    """Compute a dev-version string from ``git describe``.

    Output is the hyphenated form ``X.Y.Z-devN`` (a semver pre-release
    identifier and also valid PEP 440 input). For pull-request builds we
    append the commit SHA as semver build-metadata so concurrent PRs don't
    collide on TestPyPI.
    """
    ret = ctx.run(
        "git",
        "describe",
        "--tags",
        "--long",
        "--match",
        "v[0-9]*.[0-9]*.[0-9]*",
        capture_output=True,
        stream_output=False,
    )
    describe: str = ret.stdout.read().rstrip()  # type: ignore[assignment]
    if not describe:
        # No matching tag in history — use the fallback base.
        ret = ctx.run("git", "rev-list", "--count", "HEAD", capture_output=True, stream_output=False)
        count: str = ret.stdout.read().rstrip() or "0"  # type: ignore[assignment]
        return f"{TODAY_VERSION}-dev{count}"
    # Format: vX.Y.Z-N-gSHA  →  base=X.Y.Z, count=N, sha=gSHA
    base, count, sha = describe.split("-")
    base = base.lstrip("v")
    version_str = f"{base}-dev{count}"
    if os.environ.get("GITHUB_EVENT_NAME", "") == "pull_request":
        version_str += f"+{sha}"
    return version_str


@group.command
def current(ctx: Context) -> None:
    """Print the current `[workspace.package] version` from Cargo.toml."""
    ctx.print(_read_workspace_version())


@group.command
def bump(
    ctx: Context,
    new_version: Annotated[str | None, arg(nargs="?")] = None,
    check_existing_tag: bool = False,
    write: bool = False,
) -> None:
    """Bump the workspace version.

    Args:
        new_version: Explicit version to bump to. If omitted, a dev version is
            derived from ``git describe`` (``X.Y.Z-devN``).
        check_existing_tag: Refuse to write if a tag ``v<version>`` already
            exists (release safety net).
        write: Actually apply the bump via ``cargo set-version --workspace``;
            otherwise this is a dry-run that only prints the resolved version.
    """
    version = new_version if new_version else _compute_dev_version(ctx)

    if check_existing_tag:
        ret = ctx.run("git", "tag", "-l", f"v{version}", capture_output=True, stream_output=False)
        existing: str = ret.stdout.read().strip()  # type: ignore[assignment]
        if existing:
            ctx.error(f"Tag v{version} already exists")
            ctx.exit(1)

    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        ctx.info("Writing release-version and release-patch-name to GitHub output file ...")
        with open(github_output, "a") as f:
            f.write(f"release-version={version}\n")
            f.write(f"release-patch-name=toolr-{version}.patch\n")

    github_env = os.environ.get("GITHUB_ENV")
    if github_env:
        ctx.info("Writing RELEASE_VERSION to GitHub environment file ...")
        with open(github_env, "a") as f:
            f.write(f"RELEASE_VERSION={version}\n")

    github_step_summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if github_step_summary:
        with open(github_step_summary, "a") as f:
            f.write(f"Releasing version: `{version}`\n")

    if write:
        ctx.info(f"Setting [workspace.package] version to {version} via cargo set-version ...")
        _set_workspace_version(ctx, version)

    ctx.print(version)
