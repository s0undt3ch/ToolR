"""Tests for integer argument parsing and type casting."""

from __future__ import annotations

from typing import Annotated
from typing import Final

import pytest

from toolr import Context
from toolr._registry import CommandGroup
from toolr.utils._signature import arg

COMMAND_CHOICES: Final[tuple[int, ...]] = (80, 443, 8080, 3000)
PERCENTAGE_RANGE: Final[list[int]] = list(range(101))


@pytest.fixture
def command_group(command_group: CommandGroup) -> None:
    @command_group.command("count")
    def count_items(ctx: Context, count: int) -> None:
        """Count items.

        Args:
            ctx: The context object.
            count: The number of items to count.
        """

    @command_group.command("count-default")
    def count_items_default(ctx: Context, count: int = 10) -> None:
        """Count items with default.

        Args:
            ctx: The context object.
            count: The number of items to count.
        """

    @command_group.command("port")
    def set_port(ctx: Context, port: Annotated[int, arg(choices=COMMAND_CHOICES)]) -> None:
        """Set port.

        Args:
            ctx: The context object.
            port: The port number to set.
        """

    @command_group.command("count-metavar")
    def count_items_metavar(ctx: Context, count: Annotated[int, arg(metavar="NUMBER")]) -> None:
        """Count items with metavar.

        Args:
            ctx: The context object.
            count: The number of items to count.
        """

    @command_group.command("count-aliases")
    def count_items_aliases(ctx: Context, count: Annotated[int, arg(aliases=["--num", "-n"])] = None) -> None:
        """Count items with aliases.

        Args:
            ctx: The context object.
            count: The number of items to count.
        """

    @command_group.command("percentage")
    def set_percentage(ctx: Context, percentage: Annotated[int, arg(choices=PERCENTAGE_RANGE)]) -> None:
        """Set percentage (0-100).

        Args:
            ctx: The context object.
            percentage: The percentage value.
        """


def test_basic_integer_argument(cli_parser):
    """Test basic integer argument parsing."""
    args = cli_parser.parse_args(["test", "count", "42"])
    assert args.count == 42
    assert isinstance(args.count, int)


def test_negative_integer_argument(cli_parser):
    """Test negative integer argument parsing."""
    args = cli_parser.parse_args(["test", "count", "-42"])
    assert args.count == -42
    assert isinstance(args.count, int)


def test_zero_integer_argument(cli_parser):
    """Test zero integer argument parsing."""
    args = cli_parser.parse_args(["test", "count", "0"])
    assert args.count == 0
    assert isinstance(args.count, int)


def test_large_integer_argument(cli_parser):
    """Test large integer argument parsing."""
    large_number = "123456789"
    args = cli_parser.parse_args(["test", "count", large_number])
    assert args.count == 123456789
    assert isinstance(args.count, int)


def test_integer_with_default(cli_parser):
    """Test integer argument with default value."""
    # Test with default
    args = cli_parser.parse_args(["test", "count-default"])
    assert args.count == 10
    assert isinstance(args.count, int)

    # Test with custom value
    args = cli_parser.parse_args(["test", "count-default", "--count", "25"])
    assert args.count == 25
    assert isinstance(args.count, int)


@pytest.mark.parametrize("port", COMMAND_CHOICES)
def test_integer_with_choices(cli_parser, port):
    """Test integer argument with choices."""
    args = cli_parser.parse_args(["test", "port", str(port)])
    assert args.port == port
    assert args.port in COMMAND_CHOICES


def test_integer_with_metavar(cli_parser):
    """Test integer argument with custom metavar."""
    args = cli_parser.parse_args(["test", "count-metavar", "42"])
    assert args.count == 42
    assert isinstance(args.count, int)


@pytest.mark.parametrize("alias", ["--count", "--num", "-n"])
def test_integer_with_aliases(cli_parser, alias):
    """Test integer argument with custom aliases."""
    args = cli_parser.parse_args(["test", "count-aliases", alias, "42"])
    assert args.count == 42


def test_integer_range_validation(cli_parser):
    """Test integer argument with range validation."""
    args = cli_parser.parse_args(["test", "percentage", "75"])
    assert args.percentage == 75
    assert 0 <= args.percentage <= 100
