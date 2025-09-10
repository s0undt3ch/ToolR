"""
ToolR versioning utilities.
"""

from __future__ import annotations

import os
from typing import Annotated

from msgspec import Struct
from packaging.version import Version

from toolr import Context
from toolr import arg
from toolr import command_group

group = command_group("version", "Versioning utilities", docstring=__doc__)


class GitDescribe(Struct, frozen=True):
    """
    The result of git describe.
    """

    version: Version
    distance_to_latest_tag: int
    short_commit_hash: str


def _current_version(ctx: Context) -> str:
    ret = ctx.run("uv", "version", "--short", capture_output=True, stream_output=False)
    return Version(ret.stdout.read().rstrip())


def _git_describe(ctx: Context) -> GitDescribe:
    ret = ctx.run("git", "describe", "--tags", "--long", capture_output=True, stream_output=False)
    git_describe_output = ret.stdout.read().rstrip()
    ctx.info(f"The output of git describe is: '{git_describe_output}'")
    version, distance_to_latest_tag, short_commit_hash = git_describe_output.split("-")
    return GitDescribe(
        version=Version(version),
        distance_to_latest_tag=int(distance_to_latest_tag),
        short_commit_hash=short_commit_hash,
    )


@group.command
def current(ctx: Context) -> None:
    """
    Get the current version of ToolR.
    """
    ctx.print(_current_version(ctx))


@group.command
def bump(
    ctx: Context,
    new_version: Annotated[str | None, arg(nargs="?")],
    major: Annotated[bool, arg(group="version")] = False,
    minor: Annotated[bool, arg(group="version")] = False,
    patch: Annotated[bool, arg(group="version")] = False,
    dev: Annotated[bool, arg(group="version")] = False,
    check_existing_tag: bool = False,
    write: bool = False,
) -> None:
    """
    Bump the version of ToolR.

    Args:
        major: Whether to bump the major version.
        minor: Whether to bump the minor version.
        patch: Whether to bump the patch version.
        dev: Whether to bump the version for a development version.
        new_version: The version to bump to.
        check_existing_tag: Whether to check if the release tag already exists.
        write: Whether to write the version to the file.
    """
    if new_version is None and not any([major, minor, patch, dev]):
        ctx.error("Must pass the NEW_VERSION or one of --major/--minor/--patch/--dev")
        ctx.exit(1)
    elif new_version is not None and any([major, minor, patch, dev]):
        ctx.error("Cannot specify both NEW_VERSION and any of --major/--minor/--patch/--dev")
        ctx.exit(1)

    if new_version is not None:
        version = Version(new_version)
    else:
        gd = _git_describe(ctx)
        major_version = gd.version.major
        minor_version = gd.version.minor
        patch_version = gd.version.micro
        dev_version = ""
        if dev:
            minor_version += 1
            dev_version = f".dev{gd.distance_to_latest_tag}"
        elif major:
            major_version += 1
        elif minor:
            minor_version += 1
        elif patch:
            patch_version += 1
        else:
            ctx.error("Must specify either dev, major, minor, or patch")
            ctx.exit(1)

        version = Version(f"{major_version}.{minor_version}.{patch_version}{dev_version}")

    if check_existing_tag:
        ret = ctx.run("git", "tag", "-v", str(version), capture_output=True, stream_output=False)
        if ret.returncode == 0:
            ctx.error(f"Tag {version} already exists")
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

    if write:
        ctx.run("uv", "version", str(version))

    ctx.print(version)


@group.command
def commit(ctx: Context, version: str) -> None:
    """
    Commit the version of ToolR.

    Args:
        version: The version to commit.
    """
    ctx.run("git", "commit", "-m", f"Bump version to {version}")
    ctx.run("git", "tag", version)
