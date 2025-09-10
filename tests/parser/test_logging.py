"""Tests for parser logging setup."""

from __future__ import annotations

import sys
from unittest.mock import patch

from toolr._parser import Parser
from toolr.utils._console import ConsoleVerbosity


def test_parser_post_init_calls_setup_logging():
    """Test that Parser.__post_init__ calls setup_logging."""
    with patch("toolr._parser.setup_logging") as mock_setup_logging:
        # The patch must be applied before Parser instantiation
        Parser()

        # Should be called once during initialization
        mock_setup_logging.assert_called_once()
        call_args = mock_setup_logging.call_args
        assert "verbosity" in call_args.kwargs
        # Should be called with a ConsoleVerbosity enum value
        assert isinstance(call_args.kwargs["verbosity"], ConsoleVerbosity)


def test_parser_post_init_setup_logging_with_debug_flag():
    """Test that Parser.__post_init__ calls setup_logging with VERBOSE when debug flag is present."""
    with (
        patch("toolr._parser.setup_logging") as mock_setup_logging,
        patch.object(sys, "argv", ["toolr", "--debug", "some-command"]),
    ):
        Parser()

        mock_setup_logging.assert_called_once_with(verbosity=ConsoleVerbosity.VERBOSE)


def test_parser_post_init_setup_logging_with_quiet_flag():
    """Test that Parser.__post_init__ calls setup_logging with QUIET when quiet flag is present."""
    with (
        patch("toolr._parser.setup_logging") as mock_setup_logging,
        patch.object(sys, "argv", ["toolr", "--quiet", "some-command"]),
    ):
        Parser()

        mock_setup_logging.assert_called_once_with(verbosity=ConsoleVerbosity.QUIET)


def test_parser_post_init_setup_logging_with_normal_verbosity():
    """Test that Parser.__post_init__ calls setup_logging with NORMAL when no verbosity flags are present."""
    with (
        patch("toolr._parser.setup_logging") as mock_setup_logging,
        patch.object(sys, "argv", ["toolr", "some-command"]),
    ):
        Parser()

        mock_setup_logging.assert_called_once_with(verbosity=ConsoleVerbosity.NORMAL)


def test_parser_post_init_setup_logging_with_short_debug_flag():
    """Test that Parser.__post_init__ calls setup_logging with VERBOSE when short debug flag is present."""
    with (
        patch("toolr._parser.setup_logging") as mock_setup_logging,
        patch.object(sys, "argv", ["toolr", "-d", "some-command"]),
    ):
        Parser()

        mock_setup_logging.assert_called_once_with(verbosity=ConsoleVerbosity.VERBOSE)


def test_parser_post_init_setup_logging_with_short_quiet_flag():
    """Test that Parser.__post_init__ calls setup_logging with QUIET when short quiet flag is present."""
    with (
        patch("toolr._parser.setup_logging") as mock_setup_logging,
        patch.object(sys, "argv", ["toolr", "-q", "some-command"]),
    ):
        Parser()

        mock_setup_logging.assert_called_once_with(verbosity=ConsoleVerbosity.QUIET)
