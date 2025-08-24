"""Tests for list argument parsing and type casting."""

from __future__ import annotations

from typing import Annotated

import pytest

from toolr import Context
from toolr._registry import CommandGroup
from toolr.utils._signature import arg


@pytest.fixture
def _mutually_exclusive_group_1(command_group: CommandGroup) -> None:
    @command_group.command("mutually-exclusive-group-1")
    def group_1(
        ctx: Context,
        verbose: Annotated[bool, arg(group="verbosity")] = False,
        quiet: Annotated[bool, arg(group="verbosity")] = False,
    ) -> None:
        """Test mutually exclusive group 1.

        Args:
            ctx: The context object.
            verbose: Enable verbose output.
            quiet: Suppress all output.
        """


@pytest.mark.usefixtures("_mutually_exclusive_group_1")
def test_mutually_exclusive_groups(cli_parser):
    """Test mutually exclusive groups"""
    args = cli_parser.parse_args(["test", "mutually-exclusive-group-1"])
    assert args.verbose is False
    assert args.quiet is False

    args = cli_parser.parse_args(["test", "mutually-exclusive-group-1", "--verbose"])
    assert args.verbose is True
    assert args.quiet is False

    args = cli_parser.parse_args(["test", "mutually-exclusive-group-1", "--quiet"])
    assert args.verbose is False
    assert args.quiet is True


@pytest.mark.usefixtures("_mutually_exclusive_group_1")
def test_mutually_exclusive_groups_error(cli_parser, capfd):
    """Test mutually exclusive groups error."""
    with pytest.raises(SystemExit) as excinfo:
        cli_parser.parse_args(["test", "mutually-exclusive-group-1", "--verbose", "--quiet"])
    assert excinfo.value.code == 2
    out, err = capfd.readouterr()
    assert out == ""
    assert "error: argument --quiet: not allowed with argument --verbose" in err
