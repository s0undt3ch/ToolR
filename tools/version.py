"""Versioning utilities."""

from __future__ import annotations

import os
import re
from datetime import UTC
from datetime import datetime
from pathlib import Path
from typing import Annotated
from typing import Final

from msgspec import Struct
from packaging.version import InvalidVersion
from packaging.version import Version

from toolr import Context
from toolr import arg
from toolr import command_group

_NOW: Final[datetime] = datetime.now(UTC)
# Avoid GNU-only strftime extensions (e.g. `%-m`) — Windows' C runtime rejects
# them, which breaks module import (and pytest collection) on Windows.
TODAY_VERSION: Final[str] = f"{_NOW.year % 100}.{_NOW.month}.0"
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
        """The next development version, in normalized PEP 440 form."""
        return Version(self.next_dev_version_string)

    @property
    def next_dev_version_string(self) -> str:
        """The next development version, as a string valid for both semver and PEP 440.

        Cargo enforces semver and rejects PEP 440's `.devN` suffix (e.g.
        `0.11.0.dev42`). The hyphenated form `0.11.0-dev42` is valid both as
        a semver pre-release identifier and as PEP 440 input (which would
        normalize internally to `0.11.0.dev42`). We return the hyphenated
        form so callers writing to Cargo.toml get a value cargo accepts.
        """
        version_str = f"{self.version}-dev{self.distance_to_latest_tag}"
        if os.environ.get("GITHUB_EVENT_NAME", "") == "pull_request":
            version_str += f"+{self.short_commit_hash}"
        return version_str


CARGO_TOML_PATH: Final[Path] = Path("Cargo.toml")
# Regex matches the `version = "..."` line inside the [workspace.package]
# table. We deliberately avoid a full TOML round-trip to preserve formatting
# and comments (the toml stdlib `tomllib` is read-only; `tomlkit`/`tomli_w`
# would be additional dependencies). The block-scoped regex is anchored on
# `[workspace.package]` followed by a `version = "..."` assignment.
#
# Both anchors are pinned to start-of-line (with optional leading whitespace)
# under `re.MULTILINE`, which rejects:
#   * a commented section header like `# [workspace.package]`, and
#   * a commented version line like `# version = "0.99.0"` inside the real
#     `[workspace.package]` block.
# The `[^\[]*?` between the two anchors stops the search at the next TOML
# header so we never cross into a sibling table.
_WORKSPACE_PACKAGE_VERSION_RE: Final[re.Pattern[str]] = re.compile(
    r"(?P<prefix>^[ \t]*\[workspace\.package\][^\[]*?^[ \t]*version\s*=\s*\")"
    r"(?P<version>[^\"]+)"
    r"(?P<suffix>\")",
    re.DOTALL | re.MULTILINE,
)


def _read_workspace_version(cargo_toml: Path = CARGO_TOML_PATH) -> str:
    """Read `[workspace.package] version` out of the root `Cargo.toml`.

    Returns the raw string from the manifest (not a `packaging.Version`) so
    that callers writing it back round-trip exactly the form cargo wrote
    (cargo / semver accepts hyphenated pre-release identifiers like
    `0.11.0-dev42` which `packaging.Version` would normalize to
    `0.11.0.dev42` — a form cargo then rejects).
    """
    text = cargo_toml.read_text(encoding="utf-8")
    match = _WORKSPACE_PACKAGE_VERSION_RE.search(text)
    if match is None:
        msg = f"Could not find [workspace.package] version in {cargo_toml}"
        raise ValueError(msg)
    return match.group("version")


def _write_workspace_version(new_version: str, cargo_toml: Path = CARGO_TOML_PATH) -> None:
    """Update `[workspace.package] version` in the root `Cargo.toml`.

    `new_version` is written verbatim — callers are responsible for passing
    a string that is valid for both semver (so cargo accepts it) and PEP 440
    (so maturin/pip accept it). See `_version_str_for_cargo` for the
    validation wrapper used by `bump()`.
    """
    text = cargo_toml.read_text(encoding="utf-8")
    match = _WORKSPACE_PACKAGE_VERSION_RE.search(text)
    if match is None:
        msg = f"Could not find [workspace.package] version in {cargo_toml}"
        raise ValueError(msg)
    new_text = text[: match.start()] + match.group("prefix") + new_version + match.group("suffix") + text[match.end() :]
    cargo_toml.write_text(new_text, encoding="utf-8")


def _version_str_for_cargo(version_input: str) -> str:
    """Validate `version_input` as PEP 440 and return the original string.

    We deliberately do NOT round-trip through `str(Version(...))` because that
    normalizes `0.11.0-dev42` (semver-acceptable) to `0.11.0.dev42`
    (semver-broken). Callers should write the original input to Cargo.toml.
    """
    Version(version_input)  # raises InvalidVersion on bad input
    return version_input


def _current_version(ctx: Context) -> str:
    try:
        return _read_workspace_version()
    except (FileNotFoundError, ValueError, InvalidVersion):
        return "0.0.0"


@group.command
def current(ctx: Context) -> None:
    """Get the current version."""
    ctx.print(_current_version(ctx))


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
        version_str = ProjectVersion.discover(ctx).next_dev_version_string
    else:
        version_str = new_version

    # Validate (raises InvalidVersion on bad input) and keep the original
    # string — we must NOT round-trip through packaging.Version's normalized
    # form, which would corrupt hyphenated dev versions like `0.11.0-dev42`
    # into `0.11.0.dev42` (which cargo then rejects).
    version_str = _version_str_for_cargo(version_str)

    if check_existing_tag:
        ret = ctx.run("git", "tag", "-v", version_str, capture_output=True, stream_output=False)
        if ret.returncode == 0:
            ctx.error(f"Tag {version_str} already exists")
            ctx.exit(1)

    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        ctx.info("Writing release-version and release-patch-name to GitHub output file ...")
        with open(github_output, "a") as f:
            f.write(f"release-version={version_str}\n")
            f.write(f"release-patch-name=toolr-{version_str}.patch\n")

    github_env = os.environ.get("GITHUB_ENV")
    if github_env:
        ctx.info("Writing RELEASE_VERSION to GitHub environment file ...")
        with open(github_env, "a") as f:
            f.write(f"RELEASE_VERSION={version_str}\n")

    github_step_summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if github_step_summary:
        with open(github_step_summary, "a") as f:
            f.write(f"Releasing version: `{version_str}`\n")

    if write:
        ctx.info(f"Writing version {version_str} to [workspace.package] in {CARGO_TOML_PATH} ...")
        _write_workspace_version(version_str)

    ctx.print(version_str)
