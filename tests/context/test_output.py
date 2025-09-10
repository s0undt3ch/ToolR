"""Tests for Context output methods."""

from __future__ import annotations

from unittest import mock

from toolr._context import Context
from toolr.utils._console import Consoles
from toolr.utils._console import ConsoleVerbosity


def test_debug_output(parser, repo_root):
    """Test debug output with different verbosity levels."""
    # Test with verbose context
    verbosity = ConsoleVerbosity.VERBOSE
    consoles = Consoles.setup(verbosity)
    verbose_ctx = Context(
        parser=parser,
        repo_root=repo_root,
        verbosity=verbosity,
        _console_stderr=consoles.stderr,
        _console_stdout=consoles.stdout,
    )

    with mock.patch.object(consoles.stderr, "log") as mock_log:
        verbose_ctx.debug("debug message")
        mock_log.assert_called_once()
        call_kwargs = mock_log.call_args[1]
        assert call_kwargs["style"] == "log-debug"
        assert call_kwargs["_stack_offset"] == 2

    # Test with normal context (should not log debug)
    verbosity = ConsoleVerbosity.NORMAL
    consoles = Consoles.setup(verbosity)
    normal_ctx = Context(
        parser=parser,
        repo_root=repo_root,
        verbosity=verbosity,
        _console_stderr=consoles.stderr,
        _console_stdout=consoles.stdout,
    )

    with mock.patch.object(consoles.stderr, "log") as mock_log:
        normal_ctx.debug("debug message")
        mock_log.assert_not_called()

    # Test with quiet context (should not log debug)
    verbosity = ConsoleVerbosity.QUIET
    consoles = Consoles.setup(verbosity)
    quiet_ctx = Context(
        parser=parser,
        repo_root=repo_root,
        verbosity=verbosity,
        _console_stderr=consoles.stderr,
        _console_stdout=consoles.stdout,
    )

    with mock.patch.object(consoles.stderr, "log") as mock_log:
        quiet_ctx.debug("debug message")
        mock_log.assert_not_called()


def test_info_output(ctx):
    """Test info output."""
    with mock.patch.object(ctx._console_stderr, "log") as mock_log:
        ctx.info("info message")
        mock_log.assert_called_once()
        call_kwargs = mock_log.call_args[1]
        assert call_kwargs["style"] == "log-info"
        assert call_kwargs["_stack_offset"] == 2


def test_warn_output(ctx):
    """Test warning output."""
    with mock.patch.object(ctx._console_stderr, "log") as mock_log:
        ctx.warn("warning message")
        mock_log.assert_called_once()
        call_kwargs = mock_log.call_args[1]
        assert call_kwargs["style"] == "log-warning"
        assert call_kwargs["_stack_offset"] == 2


def test_error_output(ctx):
    """Test error output."""
    with mock.patch.object(ctx._console_stderr, "log") as mock_log:
        ctx.error("error message")
        mock_log.assert_called_once()
        call_kwargs = mock_log.call_args[1]
        assert call_kwargs["style"] == "log-error"
        assert call_kwargs["_stack_offset"] == 2


def test_print_output(ctx):
    """Test print output."""
    with mock.patch.object(ctx._console_stdout, "print") as mock_print:
        ctx.print("test message", style="bold")
        mock_print.assert_called_once_with("test message", style="bold")


def test_info_output_quiet_context(quiet_ctx):
    """Test info output with quiet context (should not log due to verbosity check)."""
    with mock.patch.object(quiet_ctx._console_stderr, "log") as mock_log:
        quiet_ctx.info("info message")
        # In quiet context, info should not be logged due to verbosity check
        mock_log.assert_not_called()


def test_warn_output_quiet_context(quiet_ctx):
    """Test warning output with quiet context (should still log)."""
    with mock.patch.object(quiet_ctx._console_stderr, "log") as mock_log:
        quiet_ctx.warn("warning message")
        mock_log.assert_called_once()
        call_kwargs = mock_log.call_args[1]
        assert call_kwargs["style"] == "log-warning"
        assert call_kwargs["_stack_offset"] == 2


def test_error_output_quiet_context(quiet_ctx):
    """Test error output with quiet context (should still log)."""
    with mock.patch.object(quiet_ctx._console_stderr, "log") as mock_log:
        quiet_ctx.error("error message")
        mock_log.assert_called_once()
        call_kwargs = mock_log.call_args[1]
        assert call_kwargs["style"] == "log-error"
        assert call_kwargs["_stack_offset"] == 2


def test_info_output_quiet_context_no_print(quiet_ctx, capfd):
    """Test info output in quiet context."""
    quiet_ctx.info("This should not be printed")

    captured = capfd.readouterr()
    assert "This should not be printed" not in captured.out
    assert "This should not be printed" not in captured.err


def test_warn_output_quiet_context_still_prints(quiet_ctx, capfd):
    """Test warn output in quiet context."""
    quiet_ctx.warn("This warning should be printed")

    captured = capfd.readouterr()
    assert "This warning should be printed" in captured.err


def test_error_output_quiet_context_still_prints(quiet_ctx, capfd):
    """Test error output in quiet context."""
    quiet_ctx.error("This error should be printed")

    captured = capfd.readouterr()
    assert "This error should be printed" in captured.err
