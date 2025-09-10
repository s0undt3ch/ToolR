"""Tests for the Parser class."""

from __future__ import annotations

import sys
from unittest.mock import patch

import pytest

from toolr._parser import Parser
from toolr.utils._console import ConsoleVerbosity


def _parameters():
    data = {
        "no arguments": (
            [],
            ConsoleVerbosity.NORMAL,
        ),
        "quiet": (
            ["-q"],
            ConsoleVerbosity.QUIET,
        ),
        "debug": (
            ["-d"],
            ConsoleVerbosity.VERBOSE,
        ),
        "debug before command": (
            ["-d", "command"],
            ConsoleVerbosity.VERBOSE,
        ),
        "debug after command": (
            ["command", "-d"],
            ConsoleVerbosity.VERBOSE,
        ),
        "quiet after command": (
            ["command", "-q"],
            ConsoleVerbosity.QUIET,
        ),
        "quiet before command": (
            ["-q", "command"],
            ConsoleVerbosity.QUIET,
        ),
    }
    return {
        "argnames": ["args", "expected_verbosity"],
        "argvalues": list(data.values()),
        "ids": [f"{k}({v[0]})" for (k, v) in data.items()],
    }


@pytest.mark.parametrize(**_parameters())
def test_parser_verbosity(args, expected_verbosity):
    """Test Parser verbosity."""
    with patch.object(sys, "argv", ["toolr", *args]):
        parser = Parser()
        # Before we call parse_args
        assert parser.context.verbosity == expected_verbosity


def test_parser_run_raises_runtime_error_before_parse_args_is_called():
    """Test that Parser raises RuntimeError before parse_args is called."""
    parser = Parser()
    with pytest.raises(RuntimeError):
        parser.run()
