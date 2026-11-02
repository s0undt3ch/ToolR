"""`CommandsTester` reports unknown groups the way the real manifest build does."""

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


@command("named-orphan")
def named(ctx):
    """No group at all."""


@command
def bare_orphan(ctx):
    """No group, bare form."""


@command(group="ci\\tx")
def tabbed(ctx):
    """Tab in the group."""


@command(group="c\\u200bi")
def zero_width(ctx):
    """Zero-width space in the group."""
'''
# Imports `z_cmds` first, so import order and file order disagree.
FIRST_MODULE = '''
from toolr import command
from tools import z_cmds  # noqa: F401


@command(group="aaa-nope")
def first(ctx):
    """First file, imported last."""
'''


def test_unknown_group_message_matches_the_manifest_build(
    make_tools_project: Callable[..., ToolsProject],
) -> None:
    """The same fixture gives the same lines, in the same order, from both."""
    project = make_tools_project(a_cmds=FIRST_MODULE, z_cmds=UNKNOWN_GROUPS_MODULE)
    proc = project.run("--help")
    assert proc.returncode != 0
    with CommandsTester(search_path=project.root) as tester:
        tester.discover()
        with pytest.raises(ValueError, match="unknown group references") as excinfo:
            tester.collected_command_groups()
    assert f"toolr: {excinfo.value}\n" in proc.stderr
