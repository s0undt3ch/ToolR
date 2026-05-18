"""Example demonstrating function name to command name conversion.

This example shows how function names with underscores are automatically converted
to command names with hyphens when decorating with a bound `@group.command`
without specifying a name.
"""

from __future__ import annotations

from toolr import Context
from toolr import command_group

names = command_group(
    "names",
    "Examples for function name to command name conversion",
    "Various examples for function name to command name conversion",
)


# Define commands using function names - they will be automatically converted
@names.command
def simple_function(ctx: Context) -> None:  # -> simple-function
    """A simple function."""


@names.command
def function_with_underscores(ctx: Context) -> None:  # -> function-with-underscores
    """A function with underscores in the name."""


@names.command
def multiple_underscores_in_name(ctx: Context) -> None:  # -> multiple-underscores-in-name
    """A function with multiple underscores."""


@names.command
def _leading_underscore(ctx: Context) -> None:  # -> -leading-underscore
    """A function with a leading underscore."""


@names.command
def trailing_underscore_(ctx: Context) -> None:  # -> trailing-underscore-
    """A function with a trailing underscore."""


@names.command
def _both_underscores_(ctx: Context) -> None:  # -> -both-underscores-
    """A function with both leading and trailing underscores."""
