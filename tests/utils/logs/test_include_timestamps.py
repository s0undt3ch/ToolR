"""Tests for include_timestamps function."""

from __future__ import annotations

import logging
from unittest.mock import patch

from toolr.utils._logs import TIMESTAMP_FORMATTER
from toolr.utils._logs import include_timestamps


def test_include_timestamps_with_timestamp_formatter():
    """Test include_timestamps when timestamp formatter is used."""
    # Create a handler with timestamp formatter
    handler = logging.StreamHandler()
    handler.setFormatter(TIMESTAMP_FORMATTER)

    with patch("logging.root.handlers", [handler]):
        assert include_timestamps() is True


def test_include_timestamps_without_timestamp_formatter():
    """Test include_timestamps when no timestamp formatter is used."""
    # Create a handler without timestamp formatter
    handler = logging.StreamHandler()
    handler.setFormatter(logging.Formatter(fmt="%(message)s"))

    with patch("logging.root.handlers", [handler]):
        assert include_timestamps() is False


def test_include_timestamps_empty_handlers():
    """Test include_timestamps with no handlers."""
    with patch("logging.root.handlers", []):
        assert include_timestamps() is False
