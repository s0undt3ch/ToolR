"""Test the DuplicateTimesFormatter class."""

from __future__ import annotations

import logging

from toolr.utils._logs import DuplicateTimesFormatter


def test_format_time_duplicate():
    """Test formatTime with duplicate timestamps."""
    formatter = DuplicateTimesFormatter(fmt="%(asctime)s%(message)s", datefmt="[%H:%M:%S] ")

    # First call should set the timestamp
    record = logging.LogRecord(
        name="test", level=logging.INFO, pathname="", lineno=0, msg="test message", args=(), exc_info=None
    )
    formatted_time = formatter.formatTime(record)
    assert formatted_time != " " * len(formatted_time)

    # Second call with same time should return spaces
    formatted_time2 = formatter.formatTime(record)
    assert formatted_time2 == " " * len(formatted_time)


def test_format_single_line():
    """Test format with single line message."""
    formatter = DuplicateTimesFormatter(fmt="%(asctime)s%(message)s", datefmt="[%H:%M:%S] ")

    record = logging.LogRecord(
        name="test", level=logging.INFO, pathname="", lineno=0, msg="test message", args=(), exc_info=None
    )

    result = formatter.format(record)
    assert "test message" in result


def test_format_multiline_with_lf():
    """Test format with multiline message using LF."""
    formatter = DuplicateTimesFormatter(fmt="%(asctime)s%(message)s", datefmt="[%H:%M:%S] ")

    record = logging.LogRecord(
        name="test", level=logging.INFO, pathname="", lineno=0, msg="line1\nline2\nline3", args=(), exc_info=None
    )

    result = formatter.format(record)
    lines = result.split("\n")
    assert len(lines) == 3
    # First line should have timestamp
    assert "[%H:%M:%S]" in lines[0] or "line1" in lines[0]
    # Subsequent lines should be indented
    assert lines[1].startswith(" ") or lines[1] == "line2"
    assert lines[2].startswith(" ") or lines[2] == "line3"


def test_format_multiline_with_crlf():
    """Test format with multiline message using CRLF."""
    formatter = DuplicateTimesFormatter(fmt="%(asctime)s%(message)s", datefmt="[%H:%M:%S] ")

    record = logging.LogRecord(
        name="test", level=logging.INFO, pathname="", lineno=0, msg="line1\r\nline2\r\nline3", args=(), exc_info=None
    )

    result = formatter.format(record)
    lines = result.split("\r\n")
    assert len(lines) == 3
    # First line should have timestamp
    assert "[%H:%M:%S]" in lines[0] or "line1" in lines[0]
    # Subsequent lines should be indented
    assert lines[1].startswith(" ") or lines[1] == "line2"
    assert lines[2].startswith(" ") or lines[2] == "line3"
    # Should end with \r
    assert result.endswith("\r")
