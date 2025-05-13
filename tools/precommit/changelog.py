"""
These commands are used to validate changelog entries.
"""

# pylint: disable=resource-leakage,broad-except,3rd-party-module-not-gated
from __future__ import annotations

import logging
import pathlib
import re

from ptscripts import Context
from ptscripts import command_group

from tools import REPO_ROOT

log = logging.getLogger(__name__)

CHANGELOG_LIKE_RE = re.compile(r"([\d]+)\.([a-z]+)$")
CHANGELOG_TYPES = (
    "security",
    "breaking",
    "deprecation",
    "feature",
    "improvement",
    "bugfix",
    "doc",
    "trivial",
)
CHANGELOG_ENTRY_RE = re.compile(r"([\d]+)\.({})(\.md)?$".format("|".join(CHANGELOG_TYPES)))

# Define the command group
changelog = command_group(
    name="changelog",
    help="Changelog tools",
    description=__doc__,
    #    venv_config=VirtualEnvPipConfig(
    #        requirements_files=[
    #            REQUIREMENTS_FILES_PATH / "changelog.txt",
    #        ],
    #    ),
    parent="pre-commit",
)


@changelog.command(
    name="pre-commit-checks",
    arguments={
        "files": {
            "nargs": "*",
        }
    },
)
def check_changelog_entries(ctx: Context, files: list[pathlib.Path]) -> None:
    """
    Run pre-commit checks on changelog snippets.
    """
    docs_path = REPO_ROOT / "doc"
    changelog_entries_path = REPO_ROOT / "changelog.d"
    exitcode = 0
    for entry in files:
        path = pathlib.Path(entry).resolve()
        # Is it under changelog/
        try:
            path.relative_to(changelog_entries_path)
            if path.name in (".gitkeep", ".template.jinja"):
                # This is the file we use so git doesn't delete the changelog/ directory
                continue
            # Is it named properly
            if not CHANGELOG_ENTRY_RE.match(path.name):
                ctx.error(
                    "The changelog entry '{}' does not match the format: '<issue-number>.({}).md'".format(
                        path.relative_to(REPO_ROOT),
                        "|".join(CHANGELOG_TYPES),
                    ),
                )
                exitcode = 1
                continue
            if path.suffix != ".md":
                ctx.error(f"Please rename '{path.relative_to(REPO_ROOT)}' to '{path.relative_to(REPO_ROOT)}.md'")
                exitcode = 1
                continue
        except ValueError:
            # No, carry on
            pass
        # Does it look like a changelog entry
        if CHANGELOG_LIKE_RE.match(path.name) and not CHANGELOG_ENTRY_RE.match(path.name):
            try:
                # Is this under doc/
                path.relative_to(docs_path)
                # Yes, carry on
                continue
            except ValueError:
                # No, resume the check
                pass
            ctx.error(
                "The changelog entry '{}' should have one of the following extensions: {}.".format(
                    path.relative_to(REPO_ROOT),
                    ", ".join(f"{ext}.md" for ext in CHANGELOG_TYPES),
                )
            )
            exitcode = 1
            continue
        # Is it a changelog entry
        if not CHANGELOG_ENTRY_RE.match(path.name):
            # No? Carry on
            continue
        # Is the changelog entry in the right path?
        try:
            path.relative_to(changelog_entries_path)
        except ValueError:
            exitcode = 1
            ctx.error(
                f"The changelog entry '{path.name}' should be placed under "
                f"'{changelog_entries_path.relative_to(REPO_ROOT)}/', "
                f"not '{path.relative_to(REPO_ROOT).parent}'"
            )
        if path.suffix != ".md":
            ctx.error(f"Please rename '{path.relative_to(REPO_ROOT)}' to '{path.relative_to(REPO_ROOT)}.md'")
            exitcode = 1
    ctx.exit(exitcode)
