"""
CI related utilities.
"""

from __future__ import annotations

import json
import os
import re

from packaging.version import Version

from toolr import Context
from toolr import command_group

group = command_group("ci", "CI utilities", docstring=__doc__)


@group.command
def generate_build_matrix(ctx: Context) -> None:
    """
    Generate a build matrix.
    """
    matrix = {
        "macos": [
            {"name": "macosx_x86_64", "os": "macos-13"},
            {"name": "macosx_arm64", "os": "macos-14"},
        ],
        "windows": [
            {"name": "win_amd64", "os": "windows-2025"},
        ],
        "linux": [
            {"name": "manylinux_x86_64", "os": "ubuntu-latest"},
            {"name": "musllinux_x86_64", "os": "ubuntu-latest"},
            {"name": "manylinux_aarch64", "os": "ubuntu-24.04-arm"},
            {"name": "musllinux_aarch64", "os": "ubuntu-24.04-arm"},
            # If we ever get a bug report asking to add s390x support, we can add it back.
            # {"name": "manylinux_s390x", "os": "ubuntu-latest", "emulation": True},
        ],
    }
    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output is None:
        ctx.error("GITHUB_OUTPUT environment variable is not set")
        ctx.exit(1)
    github_step_summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if github_step_summary is None:
        ctx.error("GITHUB_STEP_SUMMARY environment variable is not set")
        ctx.exit(1)
    with open(github_step_summary, "a") as wfh:
        wfh.write("## Build Matrix\n\n")
        wfh.write("| Platform | CI Build Wheel Image | GH Runner |\n")
        wfh.write("|----------|----------------------|-----------|\n")
        for platform, values in sorted(matrix.items()):
            for idx, item in enumerate(values):
                platform_name = platform.title() if idx == 0 else ""
                wfh.write(f"| {platform_name} | {item['name']} | {item['os']} |\n")
        wfh.write("\n")
    ctx.info("Writing build matrix to github output file ...")
    ctx.print(matrix)
    with open(github_output, "a") as f:
        f.write(f"platform-matrix={json.dumps(matrix)}\n")


@group.command
def check_run_build(ctx: Context, event_name: str, branch: str) -> None:
    """
    Check if the current build should run.

    Args:
        event_name: Event name
        branch: Branch to check for open PR
    """
    github_output = os.environ.get("GITHUB_OUTPUT")
    github_step_summary = os.environ.get("GITHUB_STEP_SUMMARY")

    if event_name == "pull_request":
        msg = "Builds for PRs should always run"
        ctx.info(msg)
        if github_step_summary is not None:
            with open(github_step_summary, "a") as wfh:
                wfh.write(f"{msg}\n")
        if github_output is not None:
            with open(github_output, "a") as wfh:
                wfh.write("should-run-build=true\n")
        ctx.exit(0)

    # This is a push event
    if branch == "main":
        msg = "Builds for the main branch should always run"
        ctx.info(msg)
        if github_step_summary is not None:
            with open(github_step_summary, "a") as wfh:
                wfh.write(f"{msg}\n")
        if github_output is not None:
            with open(github_output, "a") as wfh:
                wfh.write("should-run-build=true\n")
        ctx.exit(0)

    # This is not a push to the main branch, so, we need to check for open PRs
    ret = ctx.run(
        "gh",
        "pr",
        "list",
        "--head",
        branch,
        "--state",
        "open",
        "--json",
        "number",
        capture_output=True,
        stream_output=False,
    )
    if ret.returncode != 0:
        ctx.error("Failed to check for open PR")
        ctx.exit(1)

    prs_list = json.loads(ret.stdout.read().rstrip())
    if not prs_list:
        msg = f"Builds for branch {branch} should run since there are no open PRs"
        ctx.info(msg)
        if github_step_summary is not None:
            with open(github_step_summary, "a") as wfh:
                wfh.write(f"{msg}\n")
        if github_output is not None:
            with open(github_output, "a") as wfh:
                wfh.write("should-run-build=true\n")
        ctx.exit(0)

    pr_number = prs_list[0]["number"]
    ctx.info(f"Builds for branch/tag {branch!r} should not run since they will be built on PR #{pr_number}.")
    if github_step_summary is not None:
        github_repository = os.environ.get("GITHUB_REPOSITORY")
        if github_repository is None:
            ctx.error("GITHUB_REPOSITORY environment variable is not set")
            ctx.exit(1)
        with open(github_step_summary, "a") as wfh:
            wfh.write(
                f"Builds for branch/tag `{branch}` should not run since they will be built on "
                f"PR [#{pr_number}](https://github.com/{github_repository}/pull/{pr_number}).\n"
            )
    if github_output is not None:
        ctx.info("Updating GITHUB_OUTPUT file ...")
        with open(github_output, "a") as wfh:
            wfh.write("should-run-build=false\n")
    ctx.exit(0)


@group.command
def update_action_version(ctx: Context, version: Version) -> None:
    """
    Update the action version in 'action.yml' and on the usage of the action in the .github/ directory.

    Args:
        version: Version to update to.
    """
    exitcode = _update_action_version(ctx, version)
    ctx.exit(exitcode)


def _update_action_version(ctx: Context, version: Version) -> int:
    with open("action.yml") as rfh:
        in_contents = rfh.read().splitlines()

    # We only want to replace the first occurrence of the default version
    in_the_toolr_version_input_section = False
    out_contents = []
    for idx, line in enumerate(in_contents):
        if "description: ToolR version to install" in line:
            in_the_toolr_version_input_section = True
            out_contents.append(line)
            continue
        if in_the_toolr_version_input_section:
            out_contents.append(re.sub(r'default: "(.*)"', f'default: "{version}"', line, count=1))
            out_contents.extend(in_contents[idx + 1 :])
            # Add a blank line to the final line before the end of the file
            out_contents.append("")
            break
        out_contents.append(line)
    else:
        ctx.error("Failed to find the default version in action.yml")
        return 1

    if out_contents != in_contents:
        ctx.info(f"Updating action.yml version to {version}")
        with open("action.yml", "w") as wfh:
            wfh.write("\n".join(out_contents))

    ret = ctx.run("git", "grep", "-l", "uses: s0undt3ch/ToolR@", ".github/", capture_output=True, stream_output=False)
    if ret.returncode != 0:
        ctx.error("Failed to grep for 'uses: s0undt3ch/ToolR@' in .github/")
        return 1

    usage_version = f"v{version.major}.{version.minor}"
    for fpath in ret.stdout.read().rstrip().splitlines():
        new_uses_string = f"uses: s0undt3ch/ToolR@{usage_version}"
        with open(fpath) as rfh:
            in_contents = rfh.read()
        out_contents = re.sub(r"uses: s0undt3ch/ToolR@(.*)", new_uses_string, in_contents)
        if out_contents != in_contents:
            ctx.info(f"Updating {fpath} version to '{new_uses_string}'")
            with open(fpath, "w") as wfh:
                wfh.write(out_contents)

    return 0


def _build_rolling_tags_list(tags: list[Version]) -> list[tuple[str, Version]]:
    """
    Build the list of rolling tags that should be created/updated.

    Args:
        tags: List of version tags sorted in descending order (latest first)

    Returns:
        List of tuples containing (rolling_tag_name, target_version)
    """
    if not tags:
        return []

    rolling_tags = []
    latest_tag = tags[0]

    # Always create/update the 'latest' tag to point to the latest version
    rolling_tags.append(("latest", latest_tag))

    # Group tags by major version
    major_versions = {}
    for tag in tags:
        major_ver = tag.major
        if major_ver not in major_versions:
            major_versions[major_ver] = []
        major_versions[major_ver].append(tag)

    # For each major version, create rolling tags
    for major_ver, major_tags in major_versions.items():
        latest_major_tag = major_tags[0]  # First (latest) tag in this major version

        # Create major version rolling tag (e.g., v1, v0)
        rolling_tags.append((f"v{major_ver}", latest_major_tag))

        # Group by minor version within this major version
        minor_versions = {}
        for tag in major_tags:
            minor_ver = tag.minor
            if minor_ver not in minor_versions:
                minor_versions[minor_ver] = []
            minor_versions[minor_ver].append(tag)

        # For each minor version, create rolling tag
        for minor_ver, minor_tags in minor_versions.items():
            latest_minor_tag = minor_tags[0]  # First (latest) tag in this minor version
            # Create minor version rolling tag (e.g., v1.0, v0.10, v0.9)
            rolling_tags.append((f"v{major_ver}.{minor_ver}", latest_minor_tag))

    return rolling_tags


def _check_for_uncommitted_changes(ctx: Context) -> bool:
    """
    Check if there are any uncommitted changes to git.
    """
    ret = ctx.run("git", "status", "--porcelain", capture_output=True, stream_output=False)
    if ret.returncode != 0:
        ctx.error("Failed to check if there are any uncommitted changes to git")
        return False
    for line in ret.stdout.read().rstrip().splitlines():
        if line.strip().startswith("M"):
            return True
    return False


@group.command
def sync_rolling_tags(ctx: Context, dry_run: bool = False) -> None:
    """
    Sync rolling tags from ToolR release.

    Args:
        dry_run: Whether to dry run the command.
    """
    ret = ctx.run("git", "tag", "--list", "--sort=-version:refname", capture_output=True, stream_output=False)
    if ret.returncode != 0:
        ctx.error("Failed to get the list of tags")
        ctx.exit(1)

    tags = []
    for line in ret.stdout.read().rstrip().splitlines():
        if not line.startswith("v"):
            continue
        if not re.match(r"v[0-9]+\.[0-9]+\.[0-9]+", line):
            continue
        tags.append(Version(line))

    ctx.info("Found tags:")
    for tag in tags:
        ctx.info(f"  {tag}")

    latest_tag = tags[0]
    ctx.info("latest_tag:", latest_tag)
    exitcode = _update_action_version(ctx, latest_tag)
    if exitcode != 0:
        ctx.error(f"Failed to update to Toolr@v{latest_tag} action version")
        ctx.exit(exitcode)

    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output is not None:
        uncommitted_changes = _check_for_uncommitted_changes(ctx)
        with open(github_output, "a") as wfh:
            wfh.write(f"uncommitted-changes={str(uncommitted_changes).lower()}\n")

    # Build the list of rolling tags that should be created/updated
    rolling_tags_list = _build_rolling_tags_list(tags)

    ctx.info("Rolling tags to be created/updated:")
    for rolling_tag, target_version in rolling_tags_list:
        ctx.info(f"  {rolling_tag} -> v{target_version}")

    ctx.info("Syncing rolling tags ...")
    for rolling_tag, target_version in rolling_tags_list:
        ctx.info(f" - tag {rolling_tag} -> v{target_version}")
        if dry_run is False:
            ret = ctx.run("git", "tag", "-f", rolling_tag, f"v{target_version}")
            if ret.returncode != 0:
                ctx.error(f"Failed to tag {rolling_tag}")
                ctx.exit(1)
        ctx.info(f" - push {rolling_tag}")
        if dry_run is False:
            ret = ctx.run("git", "push", "origin", rolling_tag, "--force")
            if ret.returncode != 0:
                ctx.error(f"Failed to push {rolling_tag}")
                ctx.exit(1)

    ctx.info("Rolling tags synced successfully")
