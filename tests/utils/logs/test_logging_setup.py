"""Tests for logging setup."""

from __future__ import annotations

import os
from unittest.mock import patch

from toolr.utils._logs import NO_TIMESTAMP_FORMATTER
from toolr.utils._logs import TIMESTAMP_FORMATTER
from toolr.utils._logs import _get_default_formatter


def test_ci_environment_formatter():
    """Test that CI environment uses timestamp formatter."""
    with patch.dict(os.environ, {"CI": "true"}):
        assert _get_default_formatter() is TIMESTAMP_FORMATTER


def test_non_ci_environment_formatter():
    """Test that non-CI environment uses no timestamp formatter."""
    with patch.dict(os.environ, {}, clear=True):
        assert _get_default_formatter() is NO_TIMESTAMP_FORMATTER
