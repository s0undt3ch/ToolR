"""Versioning utilities.

Release versions are parsed with ``packaging`` (PEP 440) and rendered in two
forms:

* Cargo (SemVer), e.g. ``1.0.0-rc.1``: ``Cargo.toml``, the release tag
  (``v1.0.0-rc.1``), the GitHub Actions outputs, ``action.yml``'s default and
  the CHANGELOG.
* PEP 440, e.g. ``1.0.0rc1``: the wheels (maturin normalises the Cargo version)
  and every version comparison here.

Pre-release policy: see "Pre-releases" in ``release-notes/README.md``.
"""

from __future__ import annotations

import os
import re
import tomllib
from datetime import UTC
from datetime import datetime
from pathlib import Path
from typing import Final

from packaging.version import InvalidVersion
from packaging.version import Version

from toolr import Context
from toolr import command_group

# Built-in fallback for when there are no tags (e.g. brand-new repo).
# Avoid GNU-only strftime extensions (e.g. `%-m`) — Windows' C runtime rejects
# them, which breaks module import (and pytest collection) on Windows.
_NOW: Final[datetime] = datetime.now(UTC)
TODAY_VERSION: Final[str] = f"{_NOW.year % 100}.{_NOW.month}.0"

# `git describe --match` glob for release tags. It also matches pre-release
# tags such as `v1.0.0-rc1`; the matched tag is parsed with `packaging`.
RELEASE_TAG_GLOB: Final[str] = "v[0-9]*.[0-9]*.[0-9]*"

# SemVer spellings for PEP 440's pre-release letters. ASCII order
# (alpha < beta < rc) matches PEP 440's (a < b < rc).
_CARGO_PRE_RELEASE: Final[dict[str, str]] = {"a": "alpha", "b": "beta", "rc": "rc"}

CARGO_TOML_PATH: Final[Path] = Path("Cargo.toml")
ACTION_YML_PATH: Final[Path] = Path("action.yml")

# Surgical match for the `version:` input's `default:` line in
# action.yml. Anchors on the 2-space-indented `version:` key, then the
# *next* `default:` line at 4-or-more space indent. The non-greedy
# block in between tolerates whatever description body shape the
# action documentation grows into. See `_set_action_yml_default_version`.
_ACTION_YML_VERSION_DEFAULT_RE: Final[re.Pattern[str]] = re.compile(
    r'(\n  version:\n(?:[^\n]*\n)*?  +default: )"[^"]*"',
    re.MULTILINE,
)

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


def _set_action_yml_default_version(
    ctx: Context,
    new_version: str,
    action_yml_path: Path = ACTION_YML_PATH,
) -> None:
    """Bake ``new_version`` into action.yml's ``inputs.version.default``.

    The composite action's ``inputs.version.default`` doubles as the
    release-pin fallback when a caller pins by SHA without passing
    ``version:``. Setting it during release prep means anyone who pins
    at a release SHA gets the matching binary version automatically —
    no `version:` input needed, no SHA → tag API lookup required at
    action-run time.

    The helper writes whatever version it's given — including dev
    versions like ``X.Y.Z-devN+SHA``. The release-prep flow runs on
    every push (PRs included) via ``ci.yml``'s ``prepare-release``
    job, and exercising the bake-in regex on those runs is the
    point: catch regressions to the regex in regular CI, not only
    at real release time. The dev version never escapes the CI
    runner because PR jobs use a read-only ``GITHUB_TOKEN`` and
    cannot push the bumped action.yml back; ``release.yml`` invokes
    the same code path with an explicit ``--new-version=X.Y.Z`` so
    action.yml lands with a real tag on real releases.
    """
    text = action_yml_path.read_text(encoding="utf-8")
    new_text, count = _ACTION_YML_VERSION_DEFAULT_RE.subn(rf'\1"{new_version}"', text, count=1)
    if count != 1:
        # The release-prep workflow runs this and then commits the
        # result; a silent miss would let the bake-in regress without
        # anyone noticing. Hard-fail instead.
        ctx.error(
            f"could not locate `version:` input default in "
            f"{action_yml_path} (matched {count} times — has the "
            f"file's structure changed?)"
        )
        ctx.exit(1)
    action_yml_path.write_text(new_text, encoding="utf-8")
    ctx.info(f"Set {action_yml_path} `version` input default to '{new_version}'")


def _parse_release_version(raw: str) -> Version:
    """Parse a version to release: ``X.Y.Z`` with an optional pre-release.

    One leading ``v`` is allowed. Rejects what Cargo's ``X.Y.Z[-pre]`` can't
    hold or isn't a release: an epoch, a post-release (PEP 440 reads
    ``1.0.0-1`` as one), a dev release, a local segment, or a release segment
    that isn't three numbers.
    """
    invalid = f"{raw!r} is not a valid version"
    stripped = raw.removeprefix("v")
    # `packaging` takes a leading `v` itself, so check there was only one.
    if not stripped[:1].isdigit():
        raise ValueError(invalid)
    try:
        version = Version(stripped)
    except InvalidVersion as exc:
        raise ValueError(invalid) from exc
    if (
        len(version.release) != 3
        or version.epoch
        or version.is_postrelease
        or version.is_devrelease
        or version.local is not None
    ):
        msg = f"{raw!r} is not a release version (expected X.Y.Z or X.Y.Z with a pre-release, e.g. 1.0.0-rc.1)"
        raise ValueError(msg)
    return version


def _cargo_version(version: Version) -> str:
    """Render ``version`` in the SemVer form ``Cargo.toml`` needs.

    ``1.0.0rc2.dev3`` renders as ``1.0.0-rc.2.dev3``. maturin normalises the
    Cargo version back to PEP 440 when it builds the wheels, so the result
    must parse back to ``version``.
    """
    rendered = ".".join(str(part) for part in version.release)
    identifiers: list[str] = []
    if version.pre is not None:
        letter, number = version.pre
        identifiers += [_CARGO_PRE_RELEASE[letter], str(number)]
    if version.dev is not None:
        identifiers.append(f"dev{version.dev}")
    if identifiers:
        rendered += "-" + ".".join(identifiers)
    if version.local is not None:
        rendered += f"+{version.local}"
    return rendered


def _next_dev_version(tag: Version, count: int, sha: str | None = None) -> Version:
    """Return the dev version ``count`` commits after release ``tag``.

    After a final release ``X.Y.Z`` the dev version is ``X.Y.(Z+1).devN``;
    after a pre-release ``X.Y.Z{a,b,rc}K`` it is ``X.Y.Z{a,b,rc}(K+1).devN``.
    Either way it sorts above the tag, below the next release under PEP 440
    (which orders the wheels), and above the released version in
    ``Cargo.toml``, which ``cargo set-version`` refuses to lower.
    """
    if tag.pre is not None:
        letter, number = tag.pre
        base = f"{tag.base_version}{letter}{number + 1}"
    else:
        major, minor, patch = tag.release
        base = f"{major}.{minor}.{patch + 1}"
    local = f"+g{sha}" if sha else ""
    return Version(f"{base}.dev{count}{local}")


def _git_output(ctx: Context, *args: str) -> tuple[int, str]:
    ret = ctx.run("git", *args, capture_output=True, stream_output=False)
    stdout: str = ret.stdout.read().strip()  # type: ignore[assignment]
    return ret.returncode, stdout


def _find_release_tag(ctx: Context, version: Version) -> str | None:
    """Return the existing release tag for ``version``, whatever its spelling.

    ``v1.0.0-rc1``, ``v1.0.0rc1`` and ``v1.0.0-rc.1`` are the same release; a
    second tag for it would publish the same wheel versions again.
    """
    _, tags = _git_output(ctx, "tag", "--list", "v*")
    for tag in tags.splitlines():
        try:
            if _parse_release_version(tag) == version:
                return tag
        except ValueError:
            continue
    return None


def _compute_dev_version(ctx: Context) -> Version:
    """Compute the dev version from the latest release tag.

    The tag, the commit count since it and the SHA come from separate git
    commands, and the tag is parsed with ``packaging``; see
    :func:`_next_dev_version` for the rule. Pull-request builds append the
    commit SHA as ``+g<sha>`` so concurrent PRs don't collide on TestPyPI.
    """
    returncode, tag = _git_output(
        ctx, "describe", "--tags", "--abbrev=0", "--match", RELEASE_TAG_GLOB
    )
    if returncode != 0 or not tag:
        # No matching tag in history — use the fallback base, counting every commit.
        _, count = _git_output(ctx, "rev-list", "--count", "HEAD")
        return _next_dev_version(Version(TODAY_VERSION), int(count or "0"))
    try:
        tag_version = _parse_release_version(tag)
    except ValueError as exc:
        ctx.error(f"Cannot derive a dev version from tag {tag!r}: {exc}")
        ctx.exit(1)
    _, count = _git_output(ctx, "rev-list", "--count", f"{tag}..HEAD")
    sha = None
    if os.environ.get("GITHUB_EVENT_NAME", "") == "pull_request":
        _, sha = _git_output(ctx, "rev-parse", "--short", "HEAD")
    return _next_dev_version(tag_version, int(count), sha)


@group.command
def current(ctx: Context) -> None:
    """Print the current `[workspace.package] version` from Cargo.toml."""
    ctx.print(_read_workspace_version())


@group.command
def bump(
    ctx: Context,
    new_version: str | None = None,
    check_existing_tag: bool = False,
    include_action: bool = False,
    write: bool = False,
) -> None:
    """Bump the workspace version.

    Args:
        new_version: Explicit version to bump to: ``X.Y.Z``, optionally with a
            pre-release (``1.0.0-rc.1``, ``1.0.0rc1``). A leading ``v`` is
            accepted and stripped. Written in its Cargo form. If omitted, a dev
            version is derived from the latest release tag.
        check_existing_tag: Refuse to write if a tag for the same version
            already exists, in any spelling (release safety net).
        include_action: Also update the action.yml `version` input default.
        write: Actually apply the bump via ``cargo set-version --workspace``;
            otherwise this is a dry-run that only prints the resolved version.
    """
    if new_version:
        try:
            pep440 = _parse_release_version(new_version)
        except ValueError as exc:
            ctx.error(str(exc))
            ctx.exit(1)
    else:
        pep440 = _compute_dev_version(ctx)
    cargo_version = _cargo_version(pep440)

    if check_existing_tag:
        existing = _find_release_tag(ctx, pep440)
        if existing:
            ctx.error(f"Tag {existing} already exists for version {cargo_version}")
            ctx.exit(1)

    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        ctx.info("Writing release-version and release-patch-name to GitHub output file ...")
        with open(github_output, "a") as f:
            f.write(f"release-version={cargo_version}\n")
            f.write(f"release-patch-name=toolr-{cargo_version}.patch\n")
            # Only a pre-release segment counts, not `is_prerelease` (true for
            # any dev release): a dev build after a final release reports false,
            # so CI dry runs exercise the CHANGELOG and release-notes steps a
            # final release runs. A dev build after an rc reports true, so while
            # an rc is the latest tag, CI dry runs on main skip those steps.
            # See "Pre-releases" in release-notes/README.md.
            f.write(f"release-prerelease={str(pep440.pre is not None).lower()}\n")

    github_env = os.environ.get("GITHUB_ENV")
    if github_env:
        ctx.info("Writing RELEASE_VERSION to GitHub environment file ...")
        with open(github_env, "a") as f:
            f.write(f"RELEASE_VERSION={cargo_version}\n")

    github_step_summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if github_step_summary:
        with open(github_step_summary, "a") as f:
            f.write(f"Releasing version: `{cargo_version}`\n")

    if write:
        ctx.info(
            f"Setting [workspace.package] version to {cargo_version} via cargo set-version ..."
        )
        _set_workspace_version(ctx, cargo_version)
        if include_action:
            ctx.info(f"Updating action.yml `version` input default to '{cargo_version}' ...")
            _set_action_yml_default_version(ctx, cargo_version)

    ctx.print(cargo_version)
