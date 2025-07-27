"""Tests for LevelFilter."""

from __future__ import annotations

import logging

from toolr.utils._logs import LevelFilter


def test_level_filter_with_level():
    """Test filtering with a specific level."""
    filter_obj = LevelFilter(level=logging.INFO)

    # Create a record with INFO level
    record = logging.LogRecord(
        name="test", level=logging.INFO, pathname="", lineno=0, msg="test message", args=(), exc_info=None
    )
    assert filter_obj.filter(record) is True

    # Create a record with DEBUG level
    record = logging.LogRecord(
        name="test", level=logging.DEBUG, pathname="", lineno=0, msg="test message", args=(), exc_info=None
    )
    assert filter_obj.filter(record) is False


def test_level_filter_with_not_levels():
    """Test filtering with excluded levels."""
    filter_obj = LevelFilter(not_levels=[logging.ERROR, logging.CRITICAL])

    # Create a record with ERROR level
    record = logging.LogRecord(
        name="test", level=logging.ERROR, pathname="", lineno=0, msg="test message", args=(), exc_info=None
    )
    assert filter_obj.filter(record) is False

    # Create a record with INFO level
    record = logging.LogRecord(
        name="test", level=logging.INFO, pathname="", lineno=0, msg="test message", args=(), exc_info=None
    )
    assert filter_obj.filter(record) is True


def test_level_filter_with_both_level_and_not_levels():
    """Test filtering with both level and not_levels."""
    filter_obj = LevelFilter(level=logging.INFO, not_levels=[logging.WARNING])

    # Create a record with INFO level
    record = logging.LogRecord(
        name="test", level=logging.INFO, pathname="", lineno=0, msg="test message", args=(), exc_info=None
    )
    assert filter_obj.filter(record) is True

    # Create a record with WARNING level
    record = logging.LogRecord(
        name="test", level=logging.WARNING, pathname="", lineno=0, msg="test message", args=(), exc_info=None
    )
    assert filter_obj.filter(record) is False


def test_level_filter_with_no_constraints():
    """Test filtering with no constraints."""
    filter_obj = LevelFilter()

    # Any record should pass
    record = logging.LogRecord(
        name="test", level=logging.DEBUG, pathname="", lineno=0, msg="test message", args=(), exc_info=None
    )
    assert filter_obj.filter(record) is True

    record = logging.LogRecord(
        name="test", level=logging.WARNING, pathname="", lineno=0, msg="test message", args=(), exc_info=None
    )
    assert filter_obj.filter(record) is True
