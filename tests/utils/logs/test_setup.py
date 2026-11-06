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
# The timestamp formatter is shared by every handler and blanks a timestamp repeating the previous
# record's, so only the first handler to format a record stamps it.
TIMESTAMPED_OR_BLANKED_LINE = re.compile(r"^(\[\d{2}:\d{2}:\d{2}\] | {11})hello\n$")


@pytest.fixture(autouse=True)
def _reset_timestamp_formatter(monkeypatch):
    """Stop DuplicateTimesFormatter.format blanking the first record's timestamp.

    With `_last_timestamp` None, format() primes it itself and blanks that record; a
    never-matching value makes the first record stamped whatever ran before.
    """
    monkeypatch.setattr(TIMESTAMP_FORMATTER, "_last_timestamp", "[xx:xx:xx] ")


def test_setup_logging_verbose_level():
    """Test setup_logging with VERBOSE verbosity."""
    setup_logging(verbosity=ConsoleVerbosity.VERBOSE)
    assert logging.root.level == logging.DEBUG


def test_setup_logging_quiet_level():
    """Test setup_logging with QUIET verbosity."""
    setup_logging(verbosity=ConsoleVerbosity.QUIET)
    assert logging.root.level == logging.WARNING


def test_setup_logging_quiet_keeps_warnings_and_drops_info(caplog):
    """`-q` drops a command's `logging.info` but keeps its `logging.warning` and errors."""
    setup_logging(verbosity=ConsoleVerbosity.QUIET)
    LOGGER.info("info line")
    LOGGER.warning("warning line")
    LOGGER.error("error line")
    assert "info line" not in caplog.text
    assert "warning line" in caplog.text
    assert "error line" in caplog.text


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
    # Leave caplog's handler as the only one, so it is the first to stamp.
    monkeypatch.setattr(logging.root, "handlers", [caplog.handler])
    setup_logging(verbosity=ConsoleVerbosity.NORMAL, timestamps=True)

    assert logging.root.level == logging.INFO
    assert caplog.handler.formatter is TIMESTAMP_FORMATTER
    LOGGER.info("hello")
    assert TIMESTAMPED_LINE.match(caplog.text)


@pytest.mark.parametrize(
    "kwargs",
    [pytest.param({"timestamps": False}, id="explicit-off"), pytest.param({}, id="default-off")],
)
def test_setup_logging_without_timestamps(caplog, kwargs):
    """Timestamps are off when asked for and by default."""
    setup_logging(verbosity=ConsoleVerbosity.NORMAL, **kwargs)

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
