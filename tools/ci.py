"""
CI related utilities.
"""

from __future__ import annotations

import json
import os

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
