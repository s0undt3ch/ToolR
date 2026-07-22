"""
CI related utilities.
"""

from __future__ import annotations

import json
import os
import re
import sys
from enum import StrEnum
from pathlib import Path
from typing import Literal

from packaging.version import Version
from ruamel.yaml import YAML

from toolr import Context
from toolr import command_group

# Label name a PR can carry to opt into the full release-shaped build
# matrix. Without it, PRs run the minimal `ci` subset (native triples
# only) to keep iteration fast.
FULL_BUILD_LABEL = "full-build"

# Label name a PR can carry to opt into the full benchmark suite (macOS +
# Windows in addition to Linux). Without it, PRs bench Linux only; pushes
# to `main` always run the full suite. Mirrors FULL_BUILD_LABEL above and
# lives here so the label name sits next to the logic that reads it
# (`_run_full_bench`), rather than being hardcoded in workflow YAML.
FULL_BENCH_LABEL = "full-bench"

# tools/ci.py → repo root (two parents up resolves the `tools/` directory).
REPO_ROOT = Path(__file__).resolve().parents[1]

group = command_group("ci", "CI utilities", docstring=__doc__)


# Every supported CPython, dotted form derived below. This drives the
# *test* matrix only — the toolr-py wheel itself is now a single abi3
# build (see ABI3_WHEEL_PYTHONS). Keep aligned with `requires-python`
# in crates/toolr-py/pyproject.toml; bump when a CPython is added/removed.
ALL_CPYTHONS = ["cp311", "cp312", "cp313", "cp314"]

# CPython ABI tags cibuildwheel builds for the toolr-py (pyo3) wheel.
# abi3 stable-ABI → a single cp311 wheel covers all CPython >=3.11;
# build once, test everywhere. The pyo3 `abi3-py311` feature (root
# Cargo.toml) makes maturin tag the wheel `cp311-abi3-<platform>`, which
# pip installs on any CPython >=3.11. Building cp312/cp313/cp314 would
# only re-emit the identical abi3 wheel, so we build cp311 alone while
# TEST_PYTHONS below still exercises the full interpreter range.
ABI3_WHEEL_PYTHONS = ["cp311"]


def _cp_tag_to_dotted(tag: str) -> str:
    """Convert a cibuildwheel ABI tag back to the dotted form.

    `cp311` -> `3.11`, `cp310` -> `3.10`, etc. Used to derive the
    `_test.yml` matrix's `python-version` entries from `ALL_CPYTHONS`
    so the test matrix and the supported-CPython list can't drift.
    """
    if not tag.startswith("cp"):
        msg = f"unexpected python tag: {tag!r}"
        raise ValueError(msg)
    rest = tag[2:]
    return f"{rest[:1]}.{rest[1:]}"


# Dotted-form CPython versions for the `_test.yml` matrix. Derived from
# ALL_CPYTHONS so a single bump propagates. Stays the full range even
# though the wheel is abi3/single-build — we test every CPython against
# the one shared wheel.
TEST_PYTHONS = [_cp_tag_to_dotted(t) for t in ALL_CPYTHONS]

# The toolr binary wheel uses `bindings = "bin"` → produces a single
# py3-none-<plat> wheel per platform. One cibuildwheel invocation
# suffices regardless of CPython matrix.
BINARY_WHEEL_PYTHONS = ["cp311"]

# Per-triple metadata for the standalone toolr binary archives that
# `_build-binary-archive.yml` builds. release.yml + pushes to main ship
# all of them; PR builds use the runner-native subset (see
# `_CI_BINARY_ARCHIVE_TRIPLE_NAMES`).
#
# Every triple builds natively now: the matrix uses
# architecture-matching GitHub runners and `*-linux-musl` works as a
# native `rustup target add` + `musl-tools` build on `ubuntu-*` runners.
# No docker, no `cross`.
_BINARY_ARCHIVE_TRIPLES: list[dict[str, object]] = [
    {
        "triple": "x86_64-unknown-linux-gnu",
        "runner": "ubuntu-latest",
        "archive": "tar.gz",
        "display-name": "Linux",
    },
    {
        "triple": "aarch64-unknown-linux-gnu",
        "runner": "ubuntu-24.04-arm",
        "archive": "tar.gz",
        "display-name": "Linux",
    },
    {
        "triple": "x86_64-unknown-linux-musl",
        "runner": "ubuntu-latest",
        "archive": "tar.gz",
        "display-name": "Linux",
    },
    {
        "triple": "aarch64-unknown-linux-musl",
        "runner": "ubuntu-24.04-arm",
        "archive": "tar.gz",
        "display-name": "Linux",
    },
    {
        "triple": "aarch64-apple-darwin",
        "runner": "macos-14",
        "archive": "tar.gz",
        "display-name": "macOS",
    },
    {
        "triple": "x86_64-apple-darwin",
        "runner": "macos-15-intel",
        "archive": "tar.gz",
        "display-name": "macOS",
    },
    {
        "triple": "x86_64-pc-windows-msvc",
        "runner": "windows-latest",
        "archive": "zip",
        "display-name": "Windows",
    },
]

# Triples that ci.yml builds on every PR — one native triple per OS in
# the test matrix. Pushes to main build the full set (every triple
# release.yml would build) so main is always as close to release as
# possible.
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


def _pr_labels() -> set[str]:
    """Return the label names attached to the PR in the current GitHub event payload.

    Reads `$GITHUB_EVENT_PATH` (the action payload JSON). Returns an
    empty set when the env var is unset, the file is missing, the JSON
    is malformed, or the event isn't a `pull_request`-shaped payload.
    """
    event_path = os.environ.get("GITHUB_EVENT_PATH")
    if not event_path:
        return set()
    try:
        with open(event_path) as f:
            event = json.load(f)
    except (OSError, json.JSONDecodeError):
        return set()
    labels = event.get("pull_request", {}).get("labels", []) or []
    return {label.get("name", "") for label in labels if isinstance(label, dict)}


def _select_workflow_mode() -> tuple[Literal["ci", "release"], str]:
    """Pick `ci` vs `release` from the GitHub event context.

    Returns ``(mode, reason)``. The reason is a human-readable string
    rendered into the GitHub job step summary so reviewers can see at
    a glance *why* this run got the matrix it got.

    Rules (first match wins):

    1. ``push`` to ``refs/heads/main`` → ``release``. Main is always
       kept as close to a real release as possible so musl + cross-arch
       regressions get caught before tag-time.
    2. ``pull_request`` carrying the ``full-build`` label → ``release``.
       Opt-in escape hatch for PRs that need to exercise the full
       matrix (e.g. dependency bumps, build-system changes).
    3. Everything else → ``ci`` (the minimal native-triple subset).
    """
    event_name = os.environ.get("GITHUB_EVENT_NAME", "")
    ref = os.environ.get("GITHUB_REF", "")

    if event_name == "push" and ref == "refs/heads/main":
        return "release", "push to `main` — full matrix mirrors the release surface."

    if event_name == "pull_request":
        labels = _pr_labels()
        if FULL_BUILD_LABEL in labels:
            return (
                "release",
                f"PR carries the `{FULL_BUILD_LABEL}` label — running the full release matrix on opt-in.",
            )
        return "ci", (
            f"PR (no `{FULL_BUILD_LABEL}` label) — native-triple subset to keep PR builds snappy. "
            f"Apply the `{FULL_BUILD_LABEL}` label and re-run to opt into the full release matrix."
        )

    return (
        "ci",
        f"event=`{event_name or '(unset)'}`, ref=`{ref or '(unset)'}` — default to the minimal CI matrix.",
    )


def _run_full_bench() -> tuple[bool, str]:
    """Decide whether the full bench suite (macOS + Windows) should run.

    Reads the GitHub event context. Returns ``(run, reason)``; the reason
    is rendered into the job step
    summary so reviewers see *why* the bench shape was chosen.

    Rules (first match wins):

    1. ``push`` to ``refs/heads/main`` → run. Main mirrors the release
       surface, so cross-OS bench regressions get caught at merge time.
    2. ``pull_request`` carrying the ``full-bench`` label → run. Opt-in
       for PRs touching perf-sensitive, platform-divergent code (e.g. the
       subprocess tee/timeout loop).
    3. Everything else → Linux bench only.

    The Linux bench always runs regardless; this only gates the macOS and
    Windows bench jobs (advisory step-summary output nothing blocks on,
    and macOS runner-minutes cost ~10x Linux).
    """
    event_name = os.environ.get("GITHUB_EVENT_NAME", "")
    ref = os.environ.get("GITHUB_REF", "")

    if event_name == "push" and ref == "refs/heads/main":
        return True, "push to `main` — full bench suite (macOS + Windows)."

    if event_name == "pull_request":
        if FULL_BENCH_LABEL in _pr_labels():
            return True, f"PR carries the `{FULL_BENCH_LABEL}` label — full bench on opt-in."
        return False, (
            f"PR (no `{FULL_BENCH_LABEL}` label) — Linux bench only. "
            f"Apply the `{FULL_BENCH_LABEL}` label and re-run for the macOS + Windows suite."
        )

    return False, f"event=`{event_name or '(unset)'}` — Linux bench only."


class Workflow(StrEnum):
    """The workflow to run, either `ci` or `release`."""

    CI = "ci"
    RELEASE = "release"


@group.command
def generate_build_matrix(
    ctx: Context,
    workflow: Workflow | None = None,
) -> None:
    """
    Emit the CI matrix configuration consumed by `prepare-ci` jobs.

    Writes six GITHUB_OUTPUT keys:

      - `platform-matrix` — wheel platform map per OS (used by _build.yml).
      - `binary-archive-triples` — list of triple+runner+archive objects
        (used by _build-binary-archive.yml's matrix).
      - `pythons-binary` — CPython ABI tags for the toolr binary wheel.
      - `pythons-py` — CPython ABI tag(s) for the toolr-py (pyo3) wheel.
        A single `cp311` abi3 build: the stable-ABI wheel installs on
        every CPython >=3.11, so we build it once.
      - `test-pythons` — dotted-form CPython versions for the test matrix
        in `_test.yml` (the full supported range; every interpreter is
        tested against the single shared abi3 wheel).
      - `run-full-bench` — `true`/`false`: whether ci.yml's macOS + Windows
        bench jobs run (push to `main` or a `full-bench`-labelled PR). The
        Linux bench always runs; this only gates the other two.

    Centralising these in one place (vs. hardcoded YAML across multiple
    workflow files) keeps the binary-wheel/py-wheel/binary-archive matrices
    in sync and lets workflow files stay declarative.

    Args:
        workflow: Which matrix shape to emit. When omitted, auto-detect
            from the GitHub event context (push-to-main → release; PR
            with `full-build` label → release; otherwise → ci). Pass
            ``release`` explicitly to force the full matrix regardless
            of context — release.yml does this so tag-time builds are
            always the complete set.
    """
    if workflow is None:
        workflow, reason = _select_workflow_mode()
    else:
        reason = f"caller forced `--workflow {workflow}`."

    run_full_bench, bench_reason = _run_full_bench()

    if workflow == Workflow.RELEASE:
        binary_archive_triples: list[dict[str, object]] = list(_BINARY_ARCHIVE_TRIPLES)
    else:
        binary_archive_triples = [
            t for t in _BINARY_ARCHIVE_TRIPLES if t["triple"] in _CI_BINARY_ARCHIVE_TRIPLE_NAMES
        ]

    outputs: dict[str, object] = {
        "platform-matrix": _WHEEL_PLATFORM_MATRIX,
        "binary-archive-triples": binary_archive_triples,
        "pythons-binary": BINARY_WHEEL_PYTHONS,
        "pythons-py": ABI3_WHEEL_PYTHONS,
        "test-pythons": TEST_PYTHONS,
        "run-full-bench": run_full_bench,
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
        wfh.write(f"_{reason}_\n\n")
        wfh.write("### Wheels\n\n")
        wfh.write("| Platform | cibuildwheel platform | GH runner |\n")
        wfh.write("|----------|----------------------|-----------|\n")
        for platform, values in sorted(_WHEEL_PLATFORM_MATRIX.items()):
            for idx, item in enumerate(values):
                label = platform.title() if idx == 0 else ""
                wfh.write(f"| {label} | {item['name']} | {item['os']} |\n")
        wfh.write("\n### Standalone binary archives\n\n")
        wfh.write("| Triple | GH runner | Archive |\n")
        wfh.write("|--------|-----------|---------|\n")
        wfh.writelines(
            f"| `{t['triple']}` | {t['runner']} | `{t['archive']}` |\n"
            for t in binary_archive_triples
        )
        wfh.write("\n### Python ABIs\n\n")
        wfh.write(f"- Binary wheel: `{', '.join(BINARY_WHEEL_PYTHONS)}`\n")
        wfh.write(
            f"- toolr-py wheel: `{', '.join(ABI3_WHEEL_PYTHONS)}` "
            "(abi3 stable-ABI — one wheel covers all CPython >=3.11)\n"
        )
        wfh.write(f"- Test interpreters: `{', '.join(TEST_PYTHONS)}`\n\n")
        wfh.write("### Benchmarks\n\n")
        wfh.write(
            f"- Full suite (macOS + Windows): `{json.dumps(run_full_bench)}` — {bench_reason}\n\n"
        )

    ctx.info(f"Emitting build matrix outputs for workflow={workflow!r} (reason: {reason})")
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
            wfh.write("cargo build --release -p toolr   # ensure target/release/toolr is current\n")
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
    ctx.info(
        f"Builds for branch/tag {branch!r} should not run since they will be built on PR #{pr_number}."
    )
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
    ret = ctx.run(
        "git", "tag", "--list", "--sort=-version:refname", capture_output=True, stream_output=False
    )
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

    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output is not None:
        uncommitted_changes = _check_for_uncommitted_changes(ctx)
        with open(github_output, "a") as wfh:
            wfh.write(f"uncommitted-changes={str(uncommitted_changes).lower()}\n")


# The in-repo composite action that marks a job as a pipeline gate. Using
# the action (rather than any expression in the job) as the marker means a
# gate whose wiring was mangled or dropped is still detected — and repaired.
GATE_ACTION = "./.github/actions/pipeline-gate"

# Without `if: always()` the gate job is skipped the moment an upstream job
# fails, and a skipped check can satisfy branch protection.
GATE_JOB_IF = "always()"

# Composite actions can't read the caller's `needs` context, so the gate
# step must hand it over; the action scans it for failed/cancelled results
# and sets the exit status. The gate's `needs` must list every other job
# for that scan to be meaningful.
GATE_JOBS_INPUT = "${{ toJSON(needs) }}"


def _gate_steps(job: dict) -> list[dict]:
    """The job's steps that use the pipeline-gate action."""
    return [
        step
        for step in job.get("steps") or []
        if str(step.get("uses", "")).partition("@")[0] == GATE_ACTION
    ]


def _normalised(expression: object) -> str:
    """Collapse whitespace so folded/multi-line YAML scalars compare equal."""
    return " ".join(str(expression).split())


def _set_job_key(job: dict, key: str, value: object) -> None:
    """Set a job mapping key, inserting missing keys before `steps`.

    ruamel appends new keys at the end of a mapping, which for a job means
    after `steps` — valid YAML, unreadable layout.
    """
    if key in job:
        job[key] = value
    else:
        job.insert(list(job).index("steps"), key, value)


def _round_trip_yaml() -> YAML:
    """A ruamel round-trip loader/dumper matching this repo's workflow style."""
    yaml = YAML(typ="rt")
    yaml.preserve_quotes = True
    # Never reflow long lines (run-name expressions, comments, ...).
    yaml.width = 100_000
    # GitHub Actions house style: 2-space mappings, sequence dashes
    # indented 2 past their key.
    yaml.indent(mapping=2, sequence=4, offset=2)
    return yaml


@group.command
def check_gate_needs(ctx: Context, *, check: bool = False) -> None:
    """Keep pipeline gate jobs depending on every other job in their workflow.

    Each workflow that wants a single required status check ends with a gate
    job that fails when any of its ``needs`` reports ``failure`` or
    ``cancelled``. GitHub only exposes results for *direct* dependencies: a
    job skipped because an upstream dependency failed reports ``skipped``,
    not ``failure``, so a gate that misses even one job can go green while
    that job burns.

    Gates are detected by the action they use, not by job name: any job with
    a step that uses the in-repo ``pipeline-gate`` composite action is a
    gate. Beyond the ``needs`` list, this command enforces (and repairs) the
    rest of the gate wiring: the job-level ``if: always()``, the step's
    ``jobs: ${{ toJSON(needs) }}`` input, and that no step-level ``if``
    stops the gate from running. Workflows without a gate job are ignored.

    By default this command rewrites each gate's ``needs`` to the full job
    set (sorted, so reordering jobs in the file causes no churn; comments
    preserved), so adding a job without gating it can't land. Wired up as a
    pre-commit hook in auto-fix style: prek sees the modified workflow and
    fails the commit until the fresh file is staged.

    Args:
        check: Report drift and exit non-zero instead of rewriting files.
    """
    yaml = _round_trip_yaml()
    drifted = False
    for workflow in sorted((ctx.repo_root / ".github" / "workflows").glob("*.yml")):
        data = yaml.load(workflow.read_text())
        jobs = data.get("jobs") or {}
        gates = {job_id: job for job_id, job in jobs.items() if _gate_steps(job)}
        if not gates:
            continue
        # Gates never need each other: two gates each needing "everything
        # but me" would form a dependency cycle GitHub rejects.
        expected = sorted(job_id for job_id in jobs if job_id not in gates)
        problems = []
        for gate_id, gate in sorted(gates.items()):
            needs = gate.get("needs") or []
            needs = [needs] if isinstance(needs, str) else list(needs)
            if needs != expected:
                problems.extend(
                    f"job `{job_id}` is missing from `{gate_id}.needs` — "
                    "its failure would not fail the gate"
                    for job_id in sorted(set(expected) - set(needs))
                )
                problems.extend(
                    f"`{gate_id}.needs` lists unknown job `{job_id}`"
                    for job_id in sorted(set(needs) - set(expected))
                )
                _set_job_key(gate, "needs", expected)
            if _normalised(gate.get("if")) != GATE_JOB_IF:
                problems.append(
                    f"`{gate_id}` must carry `if: {GATE_JOB_IF}` — without it the gate "
                    "is skipped when an upstream job fails"
                )
                _set_job_key(gate, "if", GATE_JOB_IF)
            for step in _gate_steps(gate):
                if _normalised((step.get("with") or {}).get("jobs")) != GATE_JOBS_INPUT:
                    problems.append(
                        f"the `{gate_id}` gate step must pass `jobs: {GATE_JOBS_INPUT}` — "
                        "the action cannot read the `needs` context itself"
                    )
                    step.setdefault("with", {})["jobs"] = GATE_JOBS_INPUT
                if "if" in step:
                    problems.append(
                        f"the `{gate_id}` gate step must not have an `if` — "
                        "the gate must always run"
                    )
                    del step["if"]
        if problems:
            drifted = True
            for message in problems:
                ctx.error(f"{workflow.name}: {message}")
            if check:
                continue
            with workflow.open("w") as wfh:
                yaml.dump(data, wfh)
            ctx.info(f"{workflow.name}: gate wiring rewritten")
    if drifted and check:
        ctx.exit(1)
    if not drifted:
        ctx.info("All pipeline gate jobs are correctly wired.")
