"""Tests for list argument parsing and type casting."""

from __future__ import annotations

from enum import Enum
from typing import Annotated

import pytest

from toolr import Context
from toolr._registry import CommandGroup
from toolr.utils._signature import arg


class Color(Enum):
    """Color enumeration."""

    RED = "red"
    GREEN = "green"
    BLUE = "blue"


@pytest.fixture
def _star_items(command_group: CommandGroup) -> None:
    @command_group.command("star-items")
    def list_star_items(ctx: Context, *items: list[str]):
        """Test star items.

        Args:
            ctx: The context object.
            items: The list of items.
        """


@pytest.fixture
def _nargs_star_items(command_group: CommandGroup) -> None:
    @command_group.command("nargs-star-items")
    def list_nargs_star_items(ctx: Context, items: Annotated[list[str], arg(nargs="*")]) -> None:
        """Test nargs items.

        Args:
            ctx: The context object.
            items: The list of items.
        """


@pytest.fixture
def _nargs_plus_items(command_group: CommandGroup) -> None:
    @command_group.command("nargs-plus-items")
    def list_nargs_plus_items(ctx: Context, items: Annotated[list[str], arg(nargs="+")]) -> None:
        """Test nargs plus items.

        Args:
            ctx: The context object.
            items: The list of items.
        """


@pytest.fixture
def _nargs_optional_items(command_group: CommandGroup) -> None:
    @command_group.command("nargs-optional-items")
    def list_nargs_optional_items(ctx: Context, items: Annotated[str, arg(nargs="?")]) -> None:
        """Test nargs optional items.

        Args:
            ctx: The context object.
            items: The list of items.
        """


@pytest.fixture
def _nargs_integer_items(command_group: CommandGroup) -> None:
    @command_group.command("nargs-integer-items")
    def list_nargs_integer_items(ctx: Context, items: Annotated[list[str], arg(nargs=3)], count: int) -> None:
        """Test nargs integer items.

        Args:
            ctx: The context object.
            items: The list of items.
            count: The count of items.
        """


@pytest.mark.usefixtures("_star_items")
def test_star_items(cli_parser):
    """Test star items."""
    args = cli_parser.parse_args(["test", "star-items"])
    assert args.items == []
    assert isinstance(args.items, list)

    args = cli_parser.parse_args(["test", "star-items", "a", "b", "c"])
    assert args.items == ["a", "b", "c"]
    assert isinstance(args.items, list)


@pytest.mark.usefixtures("_nargs_star_items")
def test_nargs_star_items(cli_parser):
    """Test nargs star items."""
    args = cli_parser.parse_args(["test", "nargs-star-items"])
    assert args.items == []
    assert isinstance(args.items, list)

    args = cli_parser.parse_args(["test", "nargs-star-items", "a", "b", "c"])
    assert args.items == ["a", "b", "c"]
    assert isinstance(args.items, list)
    assert all(isinstance(item, str) for item in args.items)


@pytest.mark.usefixtures("_nargs_plus_items")
def test_nargs_plus_items(cli_parser, capfd):
    """Test nargs plus items."""
    # The plus argument is required
    with pytest.raises(SystemExit) as excinfo:
        cli_parser.parse_args(["test", "nargs-plus-items"])
    assert excinfo.value.code == 2
    _, err = capfd.readouterr()
    assert "error: the following arguments are required: ITEMS" in err

    args = cli_parser.parse_args(["test", "nargs-plus-items", "a", "b", "c"])
    assert args.items == ["a", "b", "c"]
    assert isinstance(args.items, list)
    assert all(isinstance(item, str) for item in args.items)


@pytest.mark.usefixtures("_nargs_optional_items")
def test_nargs_optional_items(cli_parser):
    """Test nargs optional items."""
    args = cli_parser.parse_args(["test", "nargs-optional-items", "a"])
    assert args.items == "a"


@pytest.mark.usefixtures("_nargs_integer_items")
def test_nargs_integer_items(cli_parser):
    """Test nargs integer items."""
    args = cli_parser.parse_args(["test", "nargs-integer-items", "a", "b", "c", "4"])
    assert args.items == ["a", "b", "c"]
    assert args.count == 4
    assert isinstance(args.items, list)
    assert all(isinstance(item, str) for item in args.items)
