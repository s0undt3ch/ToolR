"""Integration tests for signature parsing."""

from __future__ import annotations

import enum
from typing import Annotated

import pytest

from toolr import Context
from toolr._exc import SignatureError
from toolr.utils._signature import Arg
from toolr.utils._signature import KwArg
from toolr.utils._signature import arg
from toolr.utils._signature import get_signature


class OptionEnum(enum.Enum):
    """Test enum for choices testing."""

    OPTION1 = "option1"
    OPTION2 = "option2"
    OPTION3 = "option3"


class OutputFormat(enum.Enum):
    """Test enum for choices testing."""

    JSON = "json"
    YAML = "yaml"


def test_complex_signature_parsing():
    """Test parsing a complex function signature."""

    def test_func(
        ctx: Context,
        input_file: Annotated[str, arg(metavar="FILE")],
        output_file: Annotated[str, arg(metavar="OUTPUT")],
        out_format: Annotated[OutputFormat, arg(choices=OutputFormat)] = OutputFormat.JSON,
        verbose: bool = False,
        quiet: bool = True,
        files: list[str] | None = None,
    ) -> None:
        """Process files with various options.

        Args:
            input_file: Input file to process.
            output_file: Output file path.
            out_format: Output format.
            verbose: Enable verbose output.
            quiet: Suppress output.
            files: List of additional files.
        """

    signature = get_signature(test_func)
    assert len(signature.arguments) == 6

    # Check positional arguments
    assert signature.arguments[0].name == "input_file"
    assert signature.arguments[0].metavar == "FILE"
    assert isinstance(signature.arguments[0], Arg)

    assert signature.arguments[1].name == "output_file"
    assert signature.arguments[1].metavar == "OUTPUT"
    assert isinstance(signature.arguments[1], Arg)

    # Check keyword arguments
    assert signature.arguments[2].name == "out_format"
    assert isinstance(signature.arguments[2], KwArg)

    assert signature.arguments[3].name == "verbose"
    assert signature.arguments[3].action == "store_true"
    assert isinstance(signature.arguments[3], KwArg)

    assert signature.arguments[4].name == "quiet"
    assert signature.arguments[4].action == "store_false"
    assert isinstance(signature.arguments[4], KwArg)

    assert signature.arguments[5].name == "files"
    assert signature.arguments[5].action == "append"
    assert isinstance(signature.arguments[5], KwArg)


def test_mutually_exclusive_groups_basic():
    """Test basic mutually exclusive groups functionality."""

    def func(
        ctx: Context,
        input_file: str,
        *,
        verbose: Annotated[bool, arg(group="verbosity")] = False,
        quiet: Annotated[bool, arg(group="verbosity")] = False,
    ) -> None:
        """Test function with mutually exclusive groups.

        Args:
            input_file: Input file path.
            verbose: Enable verbose output.
            quiet: Enable quiet output.
        """

    signature = get_signature(func)
    assert len(signature.arguments) == 3

    # Check that arguments have correct group
    input_arg = signature.arguments[0]
    assert input_arg.name == "input_file"
    assert not hasattr(input_arg, "group")  # Arg doesn't have this

    verbose_arg = signature.arguments[1]
    assert verbose_arg.name == "verbose"
    assert verbose_arg.group == "verbosity"

    quiet_arg = signature.arguments[2]
    assert quiet_arg.name == "quiet"
    assert quiet_arg.group == "verbosity"


def test_mutually_exclusive_groups_error_handling():
    """Test error handling for mutually exclusive groups."""

    def func(
        ctx: Context,
        name: Annotated[str, arg(group="group1")],  # Positional with group - should error
    ) -> None:
        """Test function that should raise an error.

        Args:
            name: The name parameter.
        """

    # This should raise an error because positional arguments can't be in mutually exclusive groups
    with pytest.raises(SignatureError, match="Positional parameter 'name' cannot be in a mutually exclusive group"):
        get_signature(func)
