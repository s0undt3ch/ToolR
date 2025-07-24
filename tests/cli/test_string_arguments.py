"""Tests for string argument parsing and type casting."""

from __future__ import annotations

from typing import Annotated

import pytest

from toolr import Context
from toolr._registry import CommandGroup
from toolr.utils._signature import arg

COMMAND_CHOICES = ("hello", "world", "test")


@pytest.fixture
def command_group(command_group: CommandGroup) -> None:
    @command_group.command("echo")
    def echo_string(ctx: Context, message: str) -> None:
        """Echo a message.

        Args:
            ctx: The context object.
            message: The message to echo.
        """

    @command_group.command("echo-choices")
    def echo_string_choices(ctx: Context, message: Annotated[str, arg(choices=COMMAND_CHOICES)]) -> None:
        """Echo a message.

        Args:
            ctx: The context object.
            message: The message to echo.
        """

    @command_group.command("echo-aliases")
    def echo_string_aliases(ctx: Context, message: Annotated[str, arg(aliases=["--msg", "-m"])] = None) -> None:
        """Echo a message.

        Args:
            ctx: The context object.
            message: The message to echo.
        """


def test_basic_string_argument(cli_parser):
    """Test basic string argument parsing."""

    args = cli_parser.parse_args(["test", "echo", "hello world"])
    assert args.message == "hello world"
    assert isinstance(args.message, str)


def test_string_with_spaces(cli_parser):
    """Test string argument with spaces."""

    args = cli_parser.parse_args(["test", "echo", "hello world with spaces"])
    assert args.message == "hello world with spaces"


def test_string_with_special_characters(cli_parser):
    """Test string argument with special characters."""

    args = cli_parser.parse_args(["test", "echo", "hello@world#123"])
    assert args.message == "hello@world#123"


def test_empty_string_argument(cli_parser):
    """Test empty string argument."""

    args = cli_parser.parse_args(["test", "echo", ""])
    assert args.message == ""


@pytest.mark.parametrize("choice", COMMAND_CHOICES)
def test_string_with_choices(cli_parser, choice):
    """Test string argument with choices."""

    args = cli_parser.parse_args(["test", "echo-choices", choice])
    assert args.message == choice


@pytest.mark.parametrize("alias", ["--msg", "-m"])
def test_string_with_aliases(cli_parser, alias):
    """Test string argument with custom aliases."""
    args = cli_parser.parse_args(["test", "echo-aliases", alias, "hello"])
    assert args.message == "hello"
