"""Integration tests for signature parsing."""

from __future__ import annotations

import enum
from typing import Annotated

from toolr._context import Context
from toolr.utils._signature import Arg
from toolr.utils._signature import KwArg
from toolr.utils._signature import arg
from toolr.utils._signature import get_signature


class OptionEnum(enum.Enum):
    """Test enum for choices testing."""

    OPTION1 = "option1"
    OPTION2 = "option2"
    OPTION3 = "option3"


def test_complex_signature_parsing():
    """Test parsing a complex function signature."""

    def test_func(
        ctx: Context,
        input_file: Annotated[str, arg(metavar="FILE")],
        output_file: Annotated[str, arg(metavar="OUTPUT")],
        out_format: Annotated[OptionEnum, arg(choices=["json", "yaml"])] = OptionEnum.OPTION1,
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
    assert signature.arguments[2].choices == ["json", "yaml"]
    assert isinstance(signature.arguments[2], KwArg)

    assert signature.arguments[3].name == "verbose"
    assert signature.arguments[3].action == "store_true"
    assert isinstance(signature.arguments[3], KwArg)

    assert signature.arguments[4].name == "quiet"
    assert signature.arguments[4].action == "store_false"
    assert isinstance(signature.arguments[4], KwArg)

    assert signature.arguments[5].name == "files"
    # Note: The current implementation doesn't detect generic list types
    # for append action, only concrete list instances
    assert signature.arguments[5].action is None
    assert isinstance(signature.arguments[5], KwArg)
