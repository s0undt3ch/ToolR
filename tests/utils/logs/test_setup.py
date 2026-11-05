"""Tests for setup_logging function and related logging setup."""

from __future__ import annotations

import io
import logging
import re

import pytest

from toolr.utils._console import ConsoleVerbosity
from toolr.utils._logs import NO_TIMESTAMP_FORMATTER
from toolr.utils._logs import TIMESTAMP_FORMATTER
from toolr.utils._logs import _get_default_formatter
from toolr.utils._logs import setup_logging

LOGGER = logging.getLogger("toolr.tests.setup_logging")
TIMESTAMPED_LINE = re.compile(r"^\[\d{2}:\d{2}:\d{2}\] hello\n$")
# The timestamp formatter blanks a timestamp repeating the previous record's.
TIMESTAMPED_OR_BLANKED_LINE = re.compile(r"^(\[\d{2}:\d{2}:\d{2}\] | {11})hello\n$")


@pytest.fixture(autouse=True)
def restore_root_logging(monkeypatch):
    """Undo what setup_logging changes on the root logger and its handlers."""
    monkeypatch.setattr(logging.root, "level", logging.root.level)
    for handler in logging.root.handlers:
        monkeypatch.setattr(handler, "formatter", handler.formatter)
    # A never-matching "previous timestamp": with None, the formatter's first
    # record is always blanked, as format() primes _last_timestamp itself.
    monkeypatch.setattr(TIMESTAMP_FORMATTER, "_last_timestamp", "[xx:xx:xx] ")


def test_setup_logging_verbose_level():
    """Test setup_logging with VERBOSE verbosity."""
    setup_logging(verbosity=ConsoleVerbosity.VERBOSE)
    assert logging.root.level == logging.DEBUG


def test_setup_logging_quiet_level():
    """Test setup_logging with QUIET verbosity."""
    setup_logging(verbosity=ConsoleVerbosity.QUIET)
    assert logging.root.level == logging.CRITICAL + 1


def test_setup_logging_normal_level():
    """Test setup_logging with NORMAL verbosity."""
    setup_logging(verbosity=ConsoleVerbosity.NORMAL)
    assert logging.root.level == logging.INFO


def test_setup_logging_with_invalid_verbosity():
    """Test setup_logging with invalid verbosity falls back to NORMAL."""
    setup_logging(verbosity=999)
    assert logging.root.level == logging.INFO


def test_setup_logging_verbosity_override():
    """A later call replaces the level set by an earlier one."""
    setup_logging(verbosity=ConsoleVerbosity.QUIET)
    setup_logging(verbosity=ConsoleVerbosity.VERBOSE)
    assert logging.root.level == logging.DEBUG


def test_setup_logging_with_timestamps(caplog, monkeypatch):
    # The formatter is shared, so only the first handler to format a record
    # stamps it; leave caplog's handler as the only one.
    monkeypatch.setattr(logging.root, "handlers", [caplog.handler])
    setup_logging(verbosity=ConsoleVerbosity.NORMAL, timestamps=True)

    assert logging.root.level == logging.INFO
    assert caplog.handler.formatter is TIMESTAMP_FORMATTER
    LOGGER.info("hello")
    assert TIMESTAMPED_LINE.match(caplog.text)


def test_setup_logging_without_timestamps(caplog):
    setup_logging(verbosity=ConsoleVerbosity.NORMAL, timestamps=False)

    assert logging.root.level == logging.INFO
    assert caplog.handler.formatter is NO_TIMESTAMP_FORMATTER
    LOGGER.info("hello")
    assert caplog.text == "hello\n"


def test_setup_logging_default_timestamps(caplog):
    """Timestamps default to off."""
    setup_logging(verbosity=ConsoleVerbosity.NORMAL)

    assert logging.root.level == logging.INFO
    assert caplog.handler.formatter is NO_TIMESTAMP_FORMATTER
    LOGGER.info("hello")
    assert caplog.text == "hello\n"


def test_setup_logging_handles_all_handlers(monkeypatch):
    streams = [io.StringIO() for _ in range(3)]
    extra = [logging.StreamHandler(stream) for stream in streams]
    monkeypatch.setattr(logging.root, "handlers", [*extra, *logging.root.handlers])

    setup_logging(verbosity=ConsoleVerbosity.NORMAL, timestamps=True)

    assert logging.root.level == logging.INFO
    for handler in logging.root.handlers:
        assert handler.formatter is TIMESTAMP_FORMATTER
    LOGGER.info("hello")
    # All three share one formatter, so only the first stamps the record.
    assert TIMESTAMPED_LINE.match(streams[0].getvalue())
    for stream in streams[1:]:
        assert TIMESTAMPED_OR_BLANKED_LINE.match(stream.getvalue())


def test_setup_logging_formatter_override(caplog, monkeypatch):
    """A later call replaces the formatter set by an earlier one."""
    stream = io.StringIO()
    handler = logging.StreamHandler(stream)
    monkeypatch.setattr(logging.root, "handlers", [*logging.root.handlers, handler])

    setup_logging(verbosity=ConsoleVerbosity.NORMAL, timestamps=True)
    setup_logging(verbosity=ConsoleVerbosity.NORMAL, timestamps=False)

    assert caplog.handler.formatter is NO_TIMESTAMP_FORMATTER
    assert handler.formatter is NO_TIMESTAMP_FORMATTER
    LOGGER.info("hello")
    assert stream.getvalue() == "hello\n"
    assert caplog.text == "hello\n"


def test_ci_environment_formatter(monkeypatch):
    """CI environment uses timestamp formatter."""
    monkeypatch.setenv("CI", "true")
    assert _get_default_formatter() is TIMESTAMP_FORMATTER


def test_non_ci_environment_formatter(monkeypatch):
    """Non-CI environment uses no timestamp formatter."""
    monkeypatch.delenv("CI", raising=False)
    assert _get_default_formatter() is NO_TIMESTAMP_FORMATTER
