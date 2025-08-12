"""Tests for boolean argument parsing and type casting."""

from __future__ import annotations

from typing import Annotated

import pytest

from toolr import Context
from toolr._registry import CommandGroup
from toolr.utils._signature import arg


@pytest.fixture
def command_group(command_group: CommandGroup) -> None:
    @command_group.command("flag-false-default")
    def flag_test(ctx: Context, verbose: bool = False) -> None:
        """Test flag.

        Args:
            ctx: The context object.
            verbose: Enable verbose output.
        """

    @command_group.command("flag-true-default")
    def flag_test_true_default(ctx: Context, quiet: bool = True) -> None:
        """Test flag with True default.

        Args:
            ctx: The context object.
            quiet: Enable quiet mode.
        """

    @command_group.command("multiple-flags")
    def flags_test(ctx: Context, verbose: bool = False, quiet: bool = False, debug: bool = False) -> None:
        """Test multiple flags.

        Args:
            ctx: The context object.
            verbose: Enable verbose output.
            quiet: Enable quiet mode.
            debug: Enable debug mode.
        """

    @command_group.command("flag-aliases")
    def flag_test_aliases(ctx: Context, verbose: Annotated[bool, arg(aliases=["-v", "--verb"])] = False) -> None:
        """Test flag with aliases.

        Args:
            ctx: The context object.
            verbose: Enable verbose output.
        """

    @command_group.command("flag-custom-action")
    def flag_test_custom_action(ctx: Context, verbose: Annotated[bool, arg(action="store_true")] = False) -> None:
        """Test flag with custom action.

        Args:
            ctx: The context object.
            verbose: Enable verbose output.
        """

    @command_group.command("flag-store-false")
    def flag_test_store_false(ctx: Context, quiet: Annotated[bool, arg(action="store_false")] = True) -> None:
        """Test flag with store_false action.

        Args:
            ctx: The context object.
            quiet: Enable quiet mode.
        """


def test_basic_boolean_flag_true(cli_parser):
    """Test basic boolean flag set to True."""
    args = cli_parser.parse_args(["test", "flag-false-default", "--verbose"])
    assert args.verbose is True
    assert isinstance(args.verbose, bool)


def test_basic_boolean_flag_false(cli_parser):
    """Test basic boolean flag default (False)."""
    args = cli_parser.parse_args(["test", "flag-false-default"])
    assert args.verbose is False
    assert isinstance(args.verbose, bool)


def test_boolean_flag_with_true_default(cli_parser):
    """Test boolean flag with True default."""
    # Test default (True)
    args = cli_parser.parse_args(["test", "flag-true-default"])
    assert args.quiet is True
    assert isinstance(args.quiet, bool)

    # Test with flag (False)
    args = cli_parser.parse_args(["test", "flag-true-default", "--quiet"])
    assert args.quiet is False
    assert isinstance(args.quiet, bool)


def test_multiple_boolean_flags(cli_parser, capfd):
    """Test multiple boolean flags."""
    # Test all flags
    args = cli_parser.parse_args(["test", "multiple-flags", "--verbose", "--quiet", "--debug"])
    assert args.verbose is True
    assert args.quiet is True
    assert args.debug is True
    out, err = capfd.readouterr()
    assert not err
    assert not out

    # Test some flags
    args = cli_parser.parse_args(["test", "multiple-flags", "--verbose", "--debug"])
    assert args.verbose is True
    assert args.quiet is False
    assert args.debug is True
    out, err = capfd.readouterr()
    assert "CLI parsed options Namespace" in err
    assert not out

    # Test no flags
    args = cli_parser.parse_args(["test", "multiple-flags"])
    assert args.verbose is False
    assert args.quiet is False
    assert args.debug is False
    out, err = capfd.readouterr()
    assert "Tools executing" in err
    assert not out


@pytest.mark.parametrize("alias", ["--verbose", "--verb", "-v"])
def test_boolean_flag_with_aliases(cli_parser, alias):
    """Test boolean flag with custom aliases."""
    args = cli_parser.parse_args(["test", "flag-aliases", alias])
    assert args.verbose is True


def test_boolean_flag_with_custom_action(cli_parser):
    """Test boolean flag with custom action."""
    args = cli_parser.parse_args(["test", "flag-custom-action", "--verbose"])
    assert args.verbose is True


def test_boolean_flag_with_store_false_action(cli_parser):
    """Test boolean flag with store_false action."""
    # Test default (True)
    args = cli_parser.parse_args(["test", "flag-store-false"])
    assert args.quiet is True

    # Test with flag (False)
    args = cli_parser.parse_args(["test", "flag-store-false", "--quiet"])
    assert args.quiet is False
