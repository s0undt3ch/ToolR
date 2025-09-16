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
