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
def command_group(command_group: CommandGroup) -> None:
    @command_group.command("list")
    def list_test(ctx: Context, items: list[str] = None) -> None:
        """Test list argument.

        Args:
            ctx: The context object.
            items: The list of items.
        """

    @command_group.command("list-default")
    def list_test_default(
        ctx: Context,
        items: list[str] = ["default"],  # noqa: B006
    ) -> None:
        """Test list argument with default.

        Args:
            ctx: The context object.
            items: The list of items.
        """

    @command_group.command("list-aliases")
    def list_test_aliases(ctx: Context, items: Annotated[list[str], arg(aliases=["-i", "--item"])] = None) -> None:
        """Test list argument with aliases.

        Args:
            ctx: The context object.
            items: The list of items.
        """

    @command_group.command("list-metavar")
    def list_test_metavar(ctx: Context, items: Annotated[list[str], arg(metavar="ITEM")] = None) -> None:
        """Test list argument with metavar.

        Args:
            ctx: The context object.
            items: The list of items.
        """

    @command_group.command("list-choices")
    def list_test_choices(
        ctx: Context, items: Annotated[list[str], arg(choices=["red", "green", "blue"])] = None
    ) -> None:
        """Test list argument with choices.

        Args:
            ctx: The context object.
            items: The list of items.
        """

    @command_group.command("list-required")
    def list_test_required(ctx: Context, items: Annotated[list[str], arg(required=True)] = None) -> None:
        """Test required list argument.

        Args:
            ctx: The context object.
            items: The list of items.
        """

    @command_group.command("numbers")
    def numbers_test(ctx: Context, numbers: list[int] = None) -> None:
        """Test list of numbers.

        Args:
            ctx: The context object.
            numbers: The list of numbers.
        """

    @command_group.command("flags")
    def flags_test(ctx: Context, flags: list[bool] = None) -> None:
        """Test list of flags.

        Args:
            ctx: The context object.
            flags: The list of flags.
        """

    @command_group.command("colors")
    def colors_test(ctx: Context, colors: list[Color] = None) -> None:
        """Test list of colors.

        Args:
            ctx: The context object.
            colors: The list of colors.
        """

    @command_group.command("optional")
    def optional_test(ctx: Context, items: list[str] | None = None) -> None:
        """Test optional list.

        Args:
            ctx: The context object.
            items: The list of items.
        """

    @command_group.command("complex")
    def complex_test(
        ctx: Context,
        strings: list[str] | None = None,
        numbers: list[int] | None = None,
        flags: list[bool] = [True, False],  # noqa: B006
    ) -> None:
        """Test complex list scenario.

        Args:
            ctx: The context object.
            strings: The list of strings.
            numbers: The list of numbers.
            flags: The list of flags.
        """


def test_basic_list_argument(cli_parser):
    """Test basic list argument parsing."""
    args = cli_parser.parse_args(["test", "list", "--items", "a", "--items", "b", "--items", "c"])
    assert args.items == ["a", "b", "c"]
    assert isinstance(args.items, list)
    assert all(isinstance(item, str) for item in args.items)


def test_list_with_single_item(cli_parser):
    """Test list argument with single item."""
    args = cli_parser.parse_args(["test", "list", "--items", "single"])
    assert args.items == ["single"]
    assert isinstance(args.items, list)


def test_list_with_empty_items(cli_parser):
    """Test list argument with empty items."""
    args = cli_parser.parse_args(["test", "list", "--items", "", "--items", "not-empty", "--items", ""])
    assert args.items == ["", "not-empty", ""]
    assert isinstance(args.items, list)


def test_list_with_spaces_in_items(cli_parser):
    """Test list argument with spaces in items."""
    args = cli_parser.parse_args(
        ["test", "list", "--items", "hello world", "--items", "another item", "--items", "simple"]
    )
    assert args.items == ["hello world", "another item", "simple"]
    assert isinstance(args.items, list)


def test_list_with_default(cli_parser):
    """Test list argument with default value."""
    # Test with default
    args = cli_parser.parse_args(["test", "list-default"])
    assert args.items == ["default"]
    assert isinstance(args.items, list)

    # Test with custom values
    args = cli_parser.parse_args(["test", "list-default", "--items", "a", "--items", "b"])
    assert args.items == ["default", "a", "b"]
    assert isinstance(args.items, list)


@pytest.mark.parametrize("alias", ["--items", "--item", "-i"])
def test_list_with_aliases(cli_parser, alias):
    """Test list argument with custom aliases."""
    args = cli_parser.parse_args(["test", "list-aliases", alias, "a", alias, "b"])
    assert args.items == ["a", "b"]


def test_list_with_metavar(cli_parser):
    """Test list argument with custom metavar."""
    args = cli_parser.parse_args(["test", "list-metavar", "--items", "a", "--items", "b"])
    assert args.items == ["a", "b"]
    assert isinstance(args.items, list)


def test_list_with_choices(cli_parser):
    """Test list argument with choices."""
    args = cli_parser.parse_args(["test", "list-choices", "--items", "red", "--items", "blue"])
    assert args.items == ["red", "blue"]
    assert all(item in ["red", "green", "blue"] for item in args.items)


def test_list_required(cli_parser):
    """Test required list argument."""
    args = cli_parser.parse_args(["test", "list-required", "--items", "required", "--items", "items"])
    assert args.items == ["required", "items"]
    assert isinstance(args.items, list)


def test_list_of_integers(cli_parser):
    """Test list of integers."""
    args = cli_parser.parse_args(["test", "numbers", "--numbers", "1", "--numbers", "2", "--numbers", "3"])
    assert args.numbers == [1, 2, 3]
    assert isinstance(args.numbers, list)
    assert all(isinstance(num, int) for num in args.numbers)


def test_list_of_booleans(cli_parser):
    """Test list of booleans."""
    args = cli_parser.parse_args(["test", "flags", "--flags", "True", "--flags", "False", "--flags", "True"])
    assert isinstance(args.flags, list)
    assert all(isinstance(flag, bool) for flag in args.flags)
    assert args.flags == [True, False, True]


def test_list_of_enums(cli_parser):
    """Test list of enums."""
    args = cli_parser.parse_args(["test", "colors", "--colors", "red", "--colors", "blue"])
    assert args.colors == [Color.RED, Color.BLUE]
    assert isinstance(args.colors, list)
    assert all(isinstance(color, Color) for color in args.colors)


def test_list_with_optional_items(cli_parser):
    """Test list with optional items (list | None)."""
    # Test with items
    args = cli_parser.parse_args(["test", "optional", "--items", "a", "--items", "b"])
    assert args.items == ["a", "b"]
    assert isinstance(args.items, list)

    # Test without items
    args = cli_parser.parse_args(["test", "optional"])
    assert args.items is None


def test_complex_list_scenario(cli_parser):
    """Test complex list scenario with multiple list arguments."""
    # Test with all values
    args = cli_parser.parse_args(
        [
            "test",
            "complex",
            "--strings",
            "a",
            "--strings",
            "b",
            "--numbers",
            "1",
            "--numbers",
            "2",
            "--flags",
            "False",
            "--flags",
            "True",
        ]
    )
    assert args.strings == ["a", "b"]
    assert args.numbers == [1, 2]
    assert args.flags == [True, False, False, True]


def test_complex_list_scenario_with_defaults(cli_parser):
    """Test complex list scenario with defaults."""
    # Test with some values
    args = cli_parser.parse_args(["test", "complex", "--strings", "single"])
    assert args.strings == ["single"]
    assert args.numbers is None
    assert args.flags == [True, False]
