"""
CI related utilities.
"""

from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path

from packaging.version import Version

from toolr import Context
from toolr import command_group

# tools/ci.py → repo root (two parents up resolves the `tools/` directory).
REPO_ROOT = Path(__file__).resolve().parents[1]

group = command_group("ci", "CI utilities", docstring=__doc__)


# CPython ABI tags for the toolr-py (pyo3) wheel. Keep aligned with
# `requires-python` in crates/toolr-py/pyproject.toml and the explicit
# `[tool.cibuildwheel] build = ...` list in that same file. Bump in
# lockstep when a new CPython is added/removed.
ALL_CPYTHONS = ["cp311", "cp312", "cp313", "cp314"]

# The toolr binary wheel uses `bindings = "bin"` → produces a single
# py3-none-<plat> wheel per platform. One cibuildwheel invocation
# suffices regardless of CPython matrix.
BINARY_WHEEL_PYTHONS = ["cp311"]

# Per-triple metadata for the standalone toolr binary archives that
# `_build-binary-archive.yml` builds. release.yml ships all of them
# (downstream consumers: install.sh, mise plugin); ci.yml only needs the
# runner-native subset to back the test/docs jobs (see CI_BINARY_TRIPLES).
_BINARY_ARCHIVE_TRIPLES: list[dict[str, object]] = [
    {"triple": "x86_64-unknown-linux-gnu", "runner": "ubuntu-latest", "cross": False, "archive": "tar.gz"},
    {"triple": "aarch64-unknown-linux-gnu", "runner": "ubuntu-24.04-arm", "cross": False, "archive": "tar.gz"},
    {"triple": "x86_64-unknown-linux-musl", "runner": "ubuntu-latest", "cross": True, "archive": "tar.gz"},
    {"triple": "aarch64-unknown-linux-musl", "runner": "ubuntu-24.04-arm", "cross": True, "archive": "tar.gz"},
    {"triple": "x86_64-apple-darwin", "runner": "macos-13", "cross": False, "archive": "tar.gz"},
    {"triple": "aarch64-apple-darwin", "runner": "macos-14", "cross": False, "archive": "tar.gz"},
    {"triple": "x86_64-pc-windows-msvc", "runner": "windows-latest", "cross": False, "archive": "zip"},
]

# Triples that ci.yml builds on every PR — one native triple per OS
# in the test matrix. musl + cross-compiled aarch64 only build in
# release.yml.
_CI_BINARY_ARCHIVE_TRIPLE_NAMES = frozenset(
    {
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
    }
)

_WHEEL_PLATFORM_MATRIX: dict[str, list[dict[str, str]]] = {
    "macos": [
        {"name": "macosx_x86_64", "os": "macos-15-intel"},
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


@group.command
def generate_build_matrix(ctx: Context, workflow: str = "ci") -> None:
    """
    Emit the CI matrix configuration consumed by `prepare-ci` jobs.

    Writes four GITHUB_OUTPUT keys:

      - `platform-matrix` — wheel platform map per OS (used by _build.yml).
      - `binary-archive-triples` — list of triple+runner+cross+archive
        objects (used by _build-binary-archive.yml's matrix).
      - `pythons-binary` — CPython ABI tags for the toolr binary wheel.
      - `pythons-py` — CPython ABI tags for the toolr-py (pyo3) wheel.

    Centralising these in one place (vs. hardcoded YAML across multiple
    workflow files) keeps the binary-wheel/py-wheel/binary-archive matrices
    in sync and lets workflow files stay declarative.

    Args:
        workflow: Which workflow is asking. `"ci"` emits the native-triple
            subset (fast PR builds against linux-gnu + darwin-arm + win-msvc);
            `"release"` emits the full 7-triple matrix that ships to users.
    """
    if workflow not in ("ci", "release"):
        ctx.error(f"unknown workflow: {workflow!r} (expected 'ci' or 'release')")
        ctx.exit(1)

    if workflow == "release":
        binary_archive_triples: list[dict[str, object]] = list(_BINARY_ARCHIVE_TRIPLES)
    else:
        binary_archive_triples = [t for t in _BINARY_ARCHIVE_TRIPLES if t["triple"] in _CI_BINARY_ARCHIVE_TRIPLE_NAMES]

    outputs: dict[str, object] = {
        "platform-matrix": _WHEEL_PLATFORM_MATRIX,
        "binary-archive-triples": binary_archive_triples,
        "pythons-binary": BINARY_WHEEL_PYTHONS,
        "pythons-py": ALL_CPYTHONS,
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
        wfh.write(f"## Build matrix ({workflow})\n\n")
        wfh.write("### Wheels\n\n")
        wfh.write("| Platform | cibuildwheel platform | GH runner |\n")
        wfh.write("|----------|----------------------|-----------|\n")
        for platform, values in sorted(_WHEEL_PLATFORM_MATRIX.items()):
            for idx, item in enumerate(values):
                label = platform.title() if idx == 0 else ""
                wfh.write(f"| {label} | {item['name']} | {item['os']} |\n")
        wfh.write("\n### Standalone binary archives\n\n")
        wfh.write("| Triple | GH runner | Build mode |\n")
        wfh.write("|--------|-----------|------------|\n")
        for t in binary_archive_triples:
            mode = "cross" if t["cross"] else "native"
            wfh.write(f"| `{t['triple']}` | {t['runner']} | {mode} |\n")
        wfh.write("\n### Python ABIs\n\n")
        wfh.write(f"- Binary wheel: `{', '.join(BINARY_WHEEL_PYTHONS)}`\n")
        wfh.write(f"- toolr-py wheel: `{', '.join(ALL_CPYTHONS)}`\n\n")

    ctx.info(f"Emitting build matrix outputs for workflow={workflow!r}")
    ctx.print(outputs)
    with open(github_output, "a") as f:
        f.writelines(f"{key}={json.dumps(value)}\n" for key, value in outputs.items())


@group.command
def check_doc_snippets(ctx: Context) -> None:
    """
    Verify captured `--help` snippets under docs/ match the toolr binary.

    Runs `.pre-commit-hooks/regen-doc-snippets.py --check` and, on drift,
    writes a remediation block + the unified diff to $GITHUB_STEP_SUMMARY
    so PR reviewers see exactly what changed and how to regenerate.

    The captured stderr is also echoed to the job log for searchability.
    Honour `TOOLR_REGEN_BINARY` to pick the toolr binary used during the
    check (CI sets it to the extracted toolr-archive artifact).
    """
    script = REPO_ROOT / ".pre-commit-hooks" / "regen-doc-snippets.py"
    if not script.is_file():
        ctx.error(f"regen-doc-snippets.py not found at {script}")
        ctx.exit(1)

    result = ctx.run(str(script), "--check", capture_output=True, stream_output=False)
    stderr_text = result.stderr.read() if result.stderr is not None else ""

    # Always replay the captured stderr so the diff shows up in the
    # actions job log (searchable, copyable). Step summary is the
    # human-friendly surface; the log is the durable one.
    if stderr_text:
        sys.stderr.write(stderr_text)
        sys.stderr.flush()

    if result.returncode == 0:
        ctx.info("Doc snippets are in sync.")
        return

    github_step_summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if github_step_summary is not None:
        diff_body = stderr_text if stderr_text.endswith("\n") else stderr_text + "\n"
        with open(github_step_summary, "a") as wfh:
            wfh.write("## ❌ Doc snippets are stale\n\n")
            wfh.write(
                "The captured `--help` snippets under `docs/` no longer match "
                "the current `toolr` binary. The pre-commit hook is advisory "
                "(skipped when no `toolr` is on PATH locally), so CI is the "
                "authoritative check.\n\n"
            )
            wfh.write("**Fix locally:**\n\n")
            wfh.write("```bash\n")
            wfh.write(".pre-commit-hooks/regen-doc-snippets.py\n")
            wfh.write("git add docs/\n")
            wfh.write('git commit -m "docs: regen snippets"\n')
            wfh.write("```\n\n")
            wfh.write("<details><summary>Diff</summary>\n\n")
            wfh.write("```diff\n")
            wfh.write(diff_body)
            wfh.write("```\n\n")
            wfh.write("</details>\n")

    ctx.exit(result.returncode or 1)


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


def _update_action_version(ctx: Context, version: Version) -> int:
    ret = ctx.run("git", "grep", "-l", "uses: s0undt3ch/ToolR@", ".github/", capture_output=True, stream_output=False)
    if ret.returncode != 0:
        ctx.error("Failed to grep for 'uses: s0undt3ch/ToolR@' in .github/")
        return 1

    # Store the list of files before we reuse the ret variable
    files_to_update = ret.stdout.read().rstrip().splitlines()

    # Get the commit SHA for the version tag
    tag_name = f"v{version}"
    ret = ctx.run("git", "rev-parse", tag_name, capture_output=True, stream_output=False)
    if ret.returncode != 0:
        ctx.error(f"Failed to get commit SHA for tag {tag_name}")
        return 1
    commit_sha = ret.stdout.read().rstrip()

    usage_version = f"{commit_sha} # {tag_name}"
    for fpath in files_to_update:
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
