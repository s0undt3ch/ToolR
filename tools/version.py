"""Versioning utilities."""

from __future__ import annotations

import os
from datetime import UTC
from datetime import datetime
from typing import Annotated
from typing import Final

from msgspec import Struct
from packaging.version import InvalidVersion
from packaging.version import Version

from toolr import Context
from toolr import arg
from toolr import command_group

TODAY_VERSION: Final[str] = datetime.now(UTC).date().strftime("%y.%-m.0")
VERSION_REGEX = r"v?[0-9]{2}\.[0-9]{1,2}\.[0-9]{1,4}"

group = command_group("version", "Versioning utilities", docstring=__doc__)


class GitDescribe(Struct, frozen=True):
    """The result of git describe."""

    version: Version
    distance_to_latest_tag: int
    short_commit_hash: str

    @staticmethod
    def discover(ctx: Context) -> GitDescribe:
        """Discover the current version."""
        __discovered_version__: GitDescribe
        try:
            return GitDescribe.discover.__discovered_version__  # type: ignore[attr-defined]
        except AttributeError:
            version_str: str = TODAY_VERSION
            distance_to_latest_tag: str
            short_commit_hash: str

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
            git_describe_output: str = ret.stdout.read().rstrip()  # type: ignore[assignment]
            if not git_describe_output:
                # This happens when there are no tags
                ret = ctx.run("git", "describe", "--always", capture_output=True, stream_output=False)
                short_commit_hash = f"g{ret.stdout.read().rstrip()}"  # type: ignore[str-bytes-safe]
                ret = ctx.run("git", "rev-list", "--count", "HEAD", capture_output=True, stream_output=False)
                distance_to_latest_tag = ret.stdout.read().rstrip()  # type: ignore[assignment]
            else:
                ctx.info(f"The output of git describe is: '{git_describe_output}'")
                version_str, distance_to_latest_tag, short_commit_hash = git_describe_output.split("-")
            try:
                version = Version(version_str)
            except InvalidVersion as exc:
                ctx.warn(f"Invalid version: {exc}")
                ctx.warn(f"Using default version of {TODAY_VERSION}")
                version = Version(TODAY_VERSION)
            __discovered_version__ = GitDescribe(
                version=version,
                distance_to_latest_tag=int(distance_to_latest_tag),
                short_commit_hash=short_commit_hash,
            )
            GitDescribe.discover.__discovered_version__ = __discovered_version__  # type: ignore[attr-defined]

        return GitDescribe.discover.__discovered_version__  # type: ignore[attr-defined]


class ProjectVersion(Struct, frozen=True):
    """The version of the project."""

    version: Version
    distance_to_latest_tag: int
    short_commit_hash: str

    @staticmethod
    def discover(ctx: Context) -> ProjectVersion:
        """Discover the current version."""
        __discovered_version__: ProjectVersion
        try:
            return ProjectVersion.discover.__discovered_version__  # type: ignore[attr-defined]
        except AttributeError:
            version_str: str = TODAY_VERSION
            distance_to_latest_tag: int
            short_commit_hash: str

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
            git_describe_output: str = ret.stdout.read().rstrip()  # type: ignore[assignment]
            if not git_describe_output:
                ret = ctx.run("git", "describe", "--always", capture_output=True, stream_output=False)
                short_commit_hash = f"g{ret.stdout.read().rstrip()}"  # type: ignore[str-bytes-safe]
                ret = ctx.run("git", "rev-list", "--count", "HEAD", capture_output=True, stream_output=False)
                distance_to_latest_tag = int(ret.stdout.read().rstrip())
            else:
                ctx.info(f"The output of git describe is: '{git_describe_output}'")
                version_str, _distance_to_latest_tag, short_commit_hash = git_describe_output.split("-")
                distance_to_latest_tag = int(_distance_to_latest_tag)
            try:
                version = Version(version_str)
            except InvalidVersion as exc:
                ctx.warn(f"Invalid version: {exc}")
                ctx.warn(f"Using default version of {TODAY_VERSION}")
                version = Version(TODAY_VERSION)

            __discovered_version__ = ProjectVersion(
                version=version,
                distance_to_latest_tag=distance_to_latest_tag,
                short_commit_hash=short_commit_hash,
            )
            ProjectVersion.discover.__discovered_version__ = __discovered_version__  # type: ignore[attr-defined]

        return ProjectVersion.discover.__discovered_version__  # type: ignore[attr-defined]

    @property
    def current_version(self) -> Version:
        """The current version."""
        return self.version

    @property
    def next_dev_version(self) -> Version:
        """The next development version."""
        return Version(f"{self.version}.dev{self.distance_to_latest_tag}+{self.short_commit_hash}")


def _current_version(ctx: Context) -> Version:
    ret = ctx.run("uv", "version", "--short", capture_output=True, stream_output=False)
    try:
        return Version(ret.stdout.read().rstrip())  # type: ignore[arg-type]
    except InvalidVersion:
        return Version("0.0.0")


@group.command
def current(ctx: Context) -> None:
    """Get the current version."""
    ctx.print(str(_current_version(ctx)))


@group.command
def bump(
    ctx: Context,
    new_version: Annotated[str | None, arg(nargs="?")],
    check_existing_tag: bool = False,
    write: bool = False,
) -> None:
    """Bump the version.

    Args:
        dev: Whether to bump the version for a development version.
        new_version: The version to bump to.
        check_existing_tag: Whether to check if the release tag already exists.
        write: Whether to write the version to the file.
    """
    if new_version is None:
        version = ProjectVersion.discover(ctx).next_dev_version
    else:
        version = Version(new_version)

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

    github_step_summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if github_step_summary:
        with open(github_step_summary, "a") as f:
            f.write(f"Releasing version: `{version}`\n")

    if write:
        ctx.run("uv", "version", str(version))

    ctx.print(str(version))
