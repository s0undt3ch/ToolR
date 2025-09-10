"""Tests for setup_logging function and related logging setup."""

from __future__ import annotations

import logging
import os
from unittest.mock import patch

from toolr.utils._console import ConsoleVerbosity
from toolr.utils._logs import NO_TIMESTAMP_FORMATTER
from toolr.utils._logs import TIMESTAMP_FORMATTER
from toolr.utils._logs import _get_default_formatter
from toolr.utils._logs import setup_logging


def test_setup_logging_verbose_level():
    """Test setup_logging with VERBOSE verbosity."""
    with patch("logging.root.setLevel") as mock_set_level:
        setup_logging(verbosity=ConsoleVerbosity.VERBOSE)
        mock_set_level.assert_called_once_with(logging.DEBUG)


def test_setup_logging_quiet_level():
    """Test setup_logging with QUIET verbosity."""
    with patch("logging.root.setLevel") as mock_set_level:
        setup_logging(verbosity=ConsoleVerbosity.QUIET)
        mock_set_level.assert_called_once_with(logging.CRITICAL + 1)


def test_setup_logging_normal_level():
    """Test setup_logging with NORMAL verbosity."""
    with patch("logging.root.setLevel") as mock_set_level:
        setup_logging(verbosity=ConsoleVerbosity.NORMAL)
        mock_set_level.assert_called_once_with(logging.INFO)


def test_setup_logging_with_timestamps():
    """Test setup_logging with timestamps enabled."""
    mock_handlers = [patch("logging.StreamHandler").start() for _ in range(3)]

    with patch("logging.root.handlers", mock_handlers), patch("logging.root.setLevel") as mock_set_level:
        setup_logging(verbosity=ConsoleVerbosity.NORMAL, timestamps=True)

        # Should set level
        mock_set_level.assert_called_once_with(logging.INFO)

        # Should set formatter on all handlers
        for handler in mock_handlers:
            handler.setFormatter.assert_called_once()


def test_setup_logging_without_timestamps():
    """Test setup_logging with timestamps disabled."""
    mock_handlers = [patch("logging.StreamHandler").start() for _ in range(3)]

    with patch("logging.root.handlers", mock_handlers), patch("logging.root.setLevel") as mock_set_level:
        setup_logging(verbosity=ConsoleVerbosity.NORMAL, timestamps=False)

        # Should set level
        mock_set_level.assert_called_once_with(logging.INFO)

        # Should set formatter on all handlers
        for handler in mock_handlers:
            handler.setFormatter.assert_called_once()


def test_setup_logging_default_timestamps():
    """Test setup_logging with default timestamps parameter."""
    mock_handlers = [patch("logging.StreamHandler").start() for _ in range(3)]

    with patch("logging.root.handlers", mock_handlers), patch("logging.root.setLevel") as mock_set_level:
        setup_logging(verbosity=ConsoleVerbosity.NORMAL)

        # Should set level
        mock_set_level.assert_called_once_with(logging.INFO)

        # Should set formatter on all handlers (default timestamps=False)
        for handler in mock_handlers:
            handler.setFormatter.assert_called_once()


def test_setup_logging_handles_all_handlers():
    """Test that setup_logging affects all root handlers."""
    mock_handlers = [patch("logging.StreamHandler").start() for _ in range(5)]

    with patch("logging.root.handlers", mock_handlers), patch("logging.root.setLevel") as mock_set_level:
        setup_logging(verbosity=ConsoleVerbosity.NORMAL, timestamps=True)

        # Should set level
        mock_set_level.assert_called_once_with(logging.INFO)

        # Should set formatter on all handlers
        for handler in mock_handlers:
            handler.setFormatter.assert_called_once()


def test_setup_logging_verbosity_override():
    """Test that setup_logging properly overrides previous verbosity settings."""
    with patch("logging.root.setLevel") as mock_set_level:
        setup_logging(verbosity=ConsoleVerbosity.VERBOSE)
        mock_set_level.assert_called_once_with(logging.DEBUG)


def test_setup_logging_formatter_override():
    """Test that setup_logging properly overrides previous formatter settings."""
    mock_handlers = [patch("logging.StreamHandler").start() for _ in range(3)]

    with patch("logging.root.handlers", mock_handlers), patch("logging.root.setLevel") as mock_set_level:
        setup_logging(verbosity=ConsoleVerbosity.NORMAL, timestamps=True)

        # Should set level
        mock_set_level.assert_called_once_with(logging.INFO)

        # Should set formatter on all handlers
        for handler in mock_handlers:
            handler.setFormatter.assert_called_once()


def test_setup_logging_with_invalid_verbosity():
    """Test setup_logging with invalid verbosity falls back to NORMAL."""
    with patch("logging.root.setLevel") as mock_set_level:
        # Use an invalid verbosity value (not a ConsoleVerbosity enum)
        setup_logging(verbosity=999)

        # Should fall back to NORMAL (INFO level)
        mock_set_level.assert_called_once_with(logging.INFO)


def test_ci_environment_formatter():
    """Test that CI environment uses timestamp formatter."""
    with patch.dict(os.environ, {"CI": "true"}):
        assert _get_default_formatter() is TIMESTAMP_FORMATTER


def test_non_ci_environment_formatter():
    """Test that non-CI environment uses no timestamp formatter."""
    with patch.dict(os.environ, {}, clear=True):
        assert _get_default_formatter() is NO_TIMESTAMP_FORMATTER
