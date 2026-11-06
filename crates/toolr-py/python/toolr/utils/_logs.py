from __future__ import annotations

import json
import logging
import os
import sys
import textwrap
from functools import cache
from typing import Any

from toolr.utils._console import ConsoleVerbosity

STDOUT = sys.maxsize
logging.STDOUT = STDOUT  # type: ignore[attr-defined]
STDERR = sys.maxsize - 1
logging.STDERR = STDERR  # type: ignore[attr-defined]
logging.addLevelName(STDOUT, "STDOUT")
logging.addLevelName(STDERR, "STDERR")

# Tone down some logging handlers
logging.getLogger("markdown_it").setLevel(logging.INFO)


@cache
def _get_log_record_reserved_keywords() -> set[str]:
    record = logging.LogRecord(
        name="name",
        level=logging.INFO,
        pathname="pathname",
        lineno=0,
        msg="msg",
        args=(),
        exc_info=None,
    )
    try:
        return (
            {
                # All of the LogRecord attributes that are not private and is also not the getMessage method
                key
                for key in dir(record)
                if not key.startswith("_") and key != "getMessage"
            }
            | {
                # Python's LogRecord message attribute
                "message",
                # Standard logging formatter attributes
                "asctime",
            }
            | {
                # Let's also add rich logging specific reserved keywords
                "markup",
                "highlighter",
            }
            | {
                # And Django specific reserved keywords
                "request",
            }
        )
    finally:
        del record


class LevelFilter(logging.Filter):
    def __init__(
        self,
        level: int | None = None,
        not_levels: list[int] | tuple[int, ...] | None = None,
    ) -> None:
        self.level = level
        self.not_levels = not_levels or []

    def filter(self, record: logging.LogRecord) -> bool:
        if self.not_levels and record.levelno in self.not_levels:
            return False
        if self.level and record.levelno != self.level:  # noqa: SIM103
            return False
        return True


class ExtraFormatter(logging.Formatter):
    """Custom formatter that appends a JSON with the extra parameters to the output of the default formatter.

    Inspired on JsonFormatter.merge_record_extra
    https://github.com/nhairs/python-json-logger/blob/v3.3.0/src/pythonjsonlogger/core.py#L100-L124
    """

    def format(self, record: logging.LogRecord) -> str:
        output = super().format(record)
        extra = self._parse_extra(record)
        if len(extra) > 0:
            return output + "\n" + self._format_extra(extra)
        return output

    def _format_extra(self, extra: dict[str, Any]) -> str:
        formatted_extra = json.dumps(extra, indent=2, sort_keys=True, default=str)
        return f"  Extra:\n{textwrap.indent(formatted_extra, '    ')}"

    def _parse_extra(self, record: logging.LogRecord) -> dict[str, Any]:
        return {
            key: self._parse_value(value)
            for (key, value) in list(record.__dict__.items())
            if isinstance(key, str) and self._is_extra_key(key)
        }

    def _parse_value(self, value: Any) -> str:
        try:
            json.dumps(value)
        except TypeError:
            # Convert to strings values that can't be serialized (i.e. UUIDs)
            return str(value)
        return value

    def _is_extra_key(self, key: str) -> bool:
        return not self._is_reserved_key(key) and not self._is_private_key(key)

    def _is_reserved_key(self, key: str) -> bool:
        return key in _get_log_record_reserved_keywords()

    def _is_private_key(self, key: str) -> bool:
        return key.startswith("_")


class DuplicateTimesFormatter(ExtraFormatter):
    """
    Formatter that adds a timestamp to the message, if it's not a duplicate.
    """

    def __init__(self, *args: Any, **kwargs: Any) -> None:
        super().__init__(*args, **kwargs)
        self._last_timestamp: str | None = None

    def formatTime(  # noqa: N802
        self,
        record: logging.LogRecord,
        datefmt: str | None = None,
    ) -> str:
        formatted_time = super().formatTime(record, datefmt=datefmt)
        if self._last_timestamp and formatted_time == self._last_timestamp:
            formatted_time = " " * len(formatted_time)
        else:
            self._last_timestamp = formatted_time
        return formatted_time

    def format(self, record: logging.LogRecord) -> str:
        if "\r\n" in record.msg:
            line_split = "\r\n"
        else:
            line_split = "\n"
        lines = record.msg.replace("\r\n", "\n").splitlines()
        outlines = [lines.pop(0)]
        if self._last_timestamp:
            prefix = " " * len(self._last_timestamp)
        else:
            prefix = " " * len(self.formatTime(record, self.datefmt))
            self._last_timestamp = None
        outlines.extend([f"{prefix}{line.rstrip()}" for line in lines])
        record.msg = line_split.join(outlines).rstrip()
        if line_split.endswith("\r\n"):
            record.msg += "\r"
        return super().format(record)


class LoggingClass(logging.Logger):
    def stderr(self, msg: str, *args: Any, **kwargs: Any) -> None:
        return self.log(STDERR, msg, *args, **kwargs)

    def stdout(self, msg: str, *args: Any, **kwargs: Any) -> None:
        return self.log(STDOUT, msg, *args, **kwargs)


# Override the python's logging logger class as soon as this module is imported
if logging.getLoggerClass() is not LoggingClass:
    logging.setLoggerClass(LoggingClass)

# Reset logging handlers
logging.root.handlers.clear()
logging.root.setLevel(logging.INFO)

NO_TIMESTAMP_FORMATTER = ExtraFormatter(fmt="%(message)s")
TIMESTAMP_FORMATTER = DuplicateTimesFormatter(fmt="%(asctime)s%(message)s", datefmt="[%H:%M:%S] ")


def _get_default_formatter() -> logging.Formatter | DuplicateTimesFormatter:
    if "CI" in os.environ:
        return TIMESTAMP_FORMATTER
    return NO_TIMESTAMP_FORMATTER


DEFAULT_FORMATTER = _get_default_formatter()

STDERR_HANDLER = logging.StreamHandler(stream=sys.stderr)
STDERR_HANDLER.setLevel(STDERR)
STDERR_HANDLER.addFilter(LevelFilter(level=STDERR))

STDOUT_HANDLER = logging.StreamHandler(stream=sys.stdout)
STDOUT_HANDLER.setLevel(STDOUT)
STDOUT_HANDLER.addFilter(LevelFilter(level=STDOUT))

ROOT_HANDLER = logging.StreamHandler(stream=sys.stderr)
ROOT_HANDLER.setLevel(logging.DEBUG)
ROOT_HANDLER.addFilter(LevelFilter(not_levels=(STDERR, STDOUT)))

for handler in (ROOT_HANDLER, STDERR_HANDLER, STDOUT_HANDLER):
    handler.setFormatter(DEFAULT_FORMATTER)
    logging.root.addHandler(handler)


def include_timestamps() -> bool:
    """
    Return True if any of the configured logging handlers includes timestamps.
    """
    return any(handler.formatter is TIMESTAMP_FORMATTER for handler in logging.root.handlers)


def setup_logging(verbosity: ConsoleVerbosity, timestamps: bool = False) -> None:
    """
    Setup logging level and logging handler formatter.
    """
    match verbosity:
        case ConsoleVerbosity.VERBOSE:
            logging.root.setLevel(logging.DEBUG)
        case ConsoleVerbosity.QUIET:
            logging.root.setLevel(logging.CRITICAL + 1)
        case _:
            logging.root.setLevel(logging.INFO)

    formatter: logging.Formatter
    if timestamps:
        formatter = TIMESTAMP_FORMATTER
    else:
        formatter = NO_TIMESTAMP_FORMATTER
    for handler in logging.root.handlers:
        handler.setFormatter(formatter)
