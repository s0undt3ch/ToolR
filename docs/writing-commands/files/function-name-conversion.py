"""Example demonstrating function name to command name conversion.

This example shows how function names with underscores are automatically converted
to command names with hyphens when using the @command decorator without specifying
a name.
"""

from __future__ import annotations

from toolr import Context
from toolr import command
from toolr import command_group

command_group(
    "names",
    "Examples for function name to command name conversion",
    "Various examples for function name to command name conversion",
)


# Define commands using function names - they will be automatically converted
@command(group="names")
def simple_function(ctx: Context) -> None:  # -> simple-function
    """A simple function."""


@command(group="names")
def function_with_underscores(ctx: Context) -> None:  # -> function-with-underscores
    """A function with underscores in the name."""


@command(group="names")
def multiple_underscores_in_name(ctx: Context) -> None:  # -> multiple-underscores-in-name
    """A function with multiple underscores."""


@command(group="names")
def _leading_underscore(ctx: Context) -> None:  # -> -leading-underscore
    """A function with a leading underscore."""


@command(group="names")
def trailing_underscore_(ctx: Context) -> None:  # -> trailing-underscore-
    """A function with a trailing underscore."""


@command(group="names")
def _both_underscores_(ctx: Context) -> None:  # -> -both-underscores-
    """A function with both leading and trailing underscores."""
