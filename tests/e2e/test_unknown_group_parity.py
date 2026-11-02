"""`CommandsTester` reports unknown groups the way the real manifest build does (F-19)."""

from __future__ import annotations

from collections.abc import Callable

import pytest

from toolr.testing import CommandsTester

from tests.conftest import ToolsProject

UNKNOWN_GROUPS_MODULE = '''
from toolr import command, command_group

command_group("ci", "CI", "CI")
command_group("my_ci", "My CI", "My CI")


@command(group="zebra-tools")
def run_check(ctx):
    """Check things."""


@command(name="alpha-lint", group="cj")
def lint(ctx):
    """Lint things."""


@command(group="my-ci")
def build(ctx):
    """Build things."""


@command(group="")
def orphan(ctx):
    """Belong nowhere."""
'''


def test_unknown_group_message_matches_the_manifest_build(
    make_tools_project: Callable[..., ToolsProject],
) -> None:
    """The same fixture gives the same lines, in the same order, from both."""
    project = make_tools_project(cmds=UNKNOWN_GROUPS_MODULE)
    proc = project.run("--help")
    assert proc.returncode != 0
    with CommandsTester(search_path=project.root) as tester:
        tester.discover()
        with pytest.raises(ValueError, match="unknown group references") as excinfo:
            tester.collected_command_groups()
    assert f"toolr: {excinfo.value}\n" in proc.stderr
