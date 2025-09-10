"""Tests for ConsoleVerbosity enum and verbosity behavior."""

from __future__ import annotations

from toolr.utils._console import ConsoleVerbosity


def test_console_verbosity_repr():
    """Test ConsoleVerbosity enum repr."""
    assert repr(ConsoleVerbosity.QUIET) == "quiet"
    assert repr(ConsoleVerbosity.NORMAL) == "normal"
    assert repr(ConsoleVerbosity.VERBOSE) == "verbose"


def test_console_verbosity_comparison():
    """Test ConsoleVerbosity enum comparison."""
    assert ConsoleVerbosity.QUIET < ConsoleVerbosity.NORMAL
    assert ConsoleVerbosity.NORMAL < ConsoleVerbosity.VERBOSE
    assert ConsoleVerbosity.VERBOSE > ConsoleVerbosity.NORMAL
    assert ConsoleVerbosity.NORMAL > ConsoleVerbosity.QUIET
