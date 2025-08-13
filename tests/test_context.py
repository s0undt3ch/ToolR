"""Tests for the Context class."""

from __future__ import annotations

import os
import pathlib
import shutil
from argparse import ArgumentParser
from unittest import mock

import pytest

from toolr._context import ConsoleVerbosity
from toolr._context import Context
from toolr.utils._console import setup_consoles
from toolr.utils.command import CommandResult


@pytest.fixture
def temp_cwd(tmp_path):
    original_cwd = pathlib.Path.cwd()
    cwd = tmp_path / "cwd"
    cwd.mkdir()
    try:
        os.chdir(cwd)
        yield cwd
    finally:
        os.chdir(original_cwd)


@pytest.fixture
def repo_root(tmp_path):
    repo_root = tmp_path / "repo"
    repo_root.mkdir()
    return repo_root


@pytest.fixture
def parser():
    return ArgumentParser()


@pytest.fixture
def ctx(parser, repo_root):
    verbosity = ConsoleVerbosity.NORMAL
    console_stderr, console_stdout = setup_consoles(verbosity)
    return Context(
        parser=parser,
        repo_root=repo_root,
        verbosity=verbosity,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
    )


@pytest.fixture
def verbose_ctx(parser, repo_root):
    verbosity = ConsoleVerbosity.VERBOSE
    console_stderr, console_stdout = setup_consoles(verbosity)
    return Context(
        parser=parser,
        repo_root=repo_root,
        verbosity=verbosity,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
    )


@pytest.fixture
def quiet_ctx(parser, repo_root):
    verbosity = ConsoleVerbosity.QUIET
    console_stderr, console_stdout = setup_consoles(verbosity)
    return Context(
        parser=parser,
        repo_root=repo_root,
        verbosity=verbosity,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
    )


def test_context_frozen(ctx):
    """Test that Context is frozen."""
    with pytest.raises(AttributeError) as excinfo:
        ctx._console_stderr = None
    assert "immutable type: 'Context'" in str(excinfo.value)


def test_run_basic(ctx):
    """Test basic command execution."""
    with mock.patch("toolr.utils.command.run") as mock_run:
        args = ("echo", "hello")
        mock_run.return_value = CommandResult(args=args, stdout="output", stderr="", returncode=0)
        result = ctx.run(*args)
        mock_run.assert_called_once_with(
            ("echo", "hello"),
            stream_output=True,
            capture_output=False,
            timeout_secs=None,
            no_output_timeout_secs=None,
        )
        assert result.stdout == "output"
        assert result.returncode == 0


def test_run_with_options(ctx):
    """Test command execution with various options."""
    with mock.patch("toolr.utils.command.run") as mock_run:
        args = ("ls", "-l")
        mock_run.return_value = CommandResult(args=args, stdout="", stderr="", returncode=0)
        ctx.run(
            *args,
            stream_output=False,
            capture_output=True,
            timeout_secs=10,
            no_output_timeout_secs=5,
            custom_kwarg="value",
        )
        mock_run.assert_called_once_with(
            ("ls", "-l"),
            stream_output=False,
            capture_output=True,
            timeout_secs=10,
            no_output_timeout_secs=5,
            custom_kwarg="value",
        )


def test_chdir(ctx, temp_cwd, tmp_path):
    """Test the chdir context manager."""
    new_dir = tmp_path / "new_dir"
    new_dir.mkdir()

    with ctx.chdir(new_dir) as chdir_path:
        assert chdir_path == new_dir
        assert pathlib.Path.cwd() == new_dir

    # Should be back to original directory
    assert pathlib.Path.cwd() == temp_cwd


@pytest.mark.skip_on_windows(
    reason="[WinError 32] The process cannot access the file because it is being used by another process"
)
def test_chdir_nonexistent_original(verbose_ctx, tmp_path, capfd):
    """Test chdir when original directory doesn't exist."""
    new_cwd = tmp_path / "new_cwd"
    new_cwd.mkdir()
    os.chdir(new_cwd)

    # Create a temporary directory
    temp_dir = new_cwd / "temp_dir"
    temp_dir.mkdir()

    # Change to temp directory
    with verbose_ctx.chdir(temp_dir) as new_path:
        assert new_path == temp_dir
        assert pathlib.Path.cwd() == temp_dir

        # Remove the original cwd while we're in the temp dir
        # This simulates the case where the original cwd is deleted
        shutil.rmtree(new_cwd)

    captured = capfd.readouterr()
    assert "Unable to change back to path" in captured.err


def test_chdir_str_path(ctx, tmp_path):
    """Test chdir with string path."""
    new_dir = tmp_path / "new_dir"
    new_dir.mkdir()

    # Change to the tmp_path
    os.chdir(tmp_path)

    # Using pathlib path
    with ctx.chdir(new_dir) as chdir_path:
        assert chdir_path == new_dir
        assert pathlib.Path.cwd() == new_dir

    # Using string path
    with ctx.chdir(str(new_dir)) as chdir_path:
        assert chdir_path == new_dir
        assert pathlib.Path.cwd() == new_dir


def test_debug_output(parser, repo_root):
    """Test debug output with different verbosity levels."""
    # Test with verbose context
    verbosity = ConsoleVerbosity.VERBOSE
    console_stderr, console_stdout = setup_consoles(verbosity)
    verbose_ctx = Context(
        parser=parser,
        repo_root=repo_root,
        verbosity=verbosity,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
    )

    with mock.patch.object(console_stderr, "log") as mock_log:
        verbose_ctx.debug("debug message")
        mock_log.assert_called_once()
        call_kwargs = mock_log.call_args[1]
        assert call_kwargs["style"] == "log-debug"
        assert call_kwargs["_stack_offset"] == 2

    # Test with normal context (should not log debug)
    verbosity = ConsoleVerbosity.NORMAL
    console_stderr, console_stdout = setup_consoles(verbosity)
    normal_ctx = Context(
        parser=parser,
        repo_root=repo_root,
        verbosity=verbosity,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
    )

    with mock.patch.object(console_stderr, "log") as mock_log:
        normal_ctx.debug("debug message")
        mock_log.assert_not_called()

    # Test with quiet context (should not log debug)
    verbosity = ConsoleVerbosity.QUIET
    console_stderr, console_stdout = setup_consoles(verbosity)
    quiet_ctx = Context(
        parser=parser,
        repo_root=repo_root,
        verbosity=verbosity,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
    )

    with mock.patch.object(console_stderr, "log") as mock_log:
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


def test_exit_with_message(ctx, capfd):
    """Test exit with message."""
    with pytest.raises(SystemExit) as exc_info:
        ctx.exit(1, "error message")
    assert exc_info.value.code == 1
    captured = capfd.readouterr()
    assert "error message" in captured.err


def test_exit_without_message(ctx, capfd):
    """Test exit without message."""
    with pytest.raises(SystemExit) as exc_info:
        ctx.exit(0)
    assert exc_info.value.code == 0
    captured = capfd.readouterr()
    assert captured.out == ""
    assert captured.err == ""


def test_print_output(ctx):
    """Test print output."""
    with mock.patch.object(ctx._console_stdout, "print") as mock_print:
        ctx.print("test message", style="bold")
        mock_print.assert_called_once_with("test message", style="bold")


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


def test_exit_with_success_message(verbose_ctx, capfd):
    """Test exit method with a success message."""
    with pytest.raises(SystemExit) as exc_info:
        verbose_ctx.exit(0, "Success message")

    assert exc_info.value.code == 0
    captured = capfd.readouterr()
    # Exit messages go to stderr, not stdout
    assert "Success message" in captured.err


def test_exit_with_error_message(verbose_ctx, capfd):
    """Test exit method with an error message."""
    with pytest.raises(SystemExit) as exc_info:
        verbose_ctx.exit(1, "Error message")

    assert exc_info.value.code == 1
    captured = capfd.readouterr()
    # Exit messages go to stderr, not stdout
    assert "Error message" in captured.err


def test_exit_with_custom_code(verbose_ctx):
    """Test exit method with a custom exit code."""
    with pytest.raises(SystemExit) as exc_info:
        verbose_ctx.exit(42)

    assert exc_info.value.code == 42


def test_run_command_basic(verbose_ctx, capfd):
    """Test run method with basic command."""
    args = ("echo", "hello")
    command_result = CommandResult(args=args, stdout="output", stderr="", returncode=0)
    with mock.patch("toolr.utils.command.run", return_value=command_result):
        result = verbose_ctx.run(*args)
        assert result == command_result

    # We assert separately because rich will colorize the output
    captured = capfd.readouterr()
    assert "Running" in captured.err
    assert "echo hello" in captured.err


def test_run_command_with_options(verbose_ctx, capfd):
    """Test run method with various options."""
    args = ("test", "command")
    command_result = CommandResult(args=args, stdout="test output", stderr="", returncode=0)

    def mock_run(*args, **kwargs):
        # Verify the options are passed correctly
        assert kwargs.get("stream_output") is False
        assert kwargs.get("capture_output") is True
        assert kwargs.get("timeout_secs") == 30.0
        assert kwargs.get("no_output_timeout_secs") == 60.0
        return command_result

    with mock.patch("toolr.utils.command.run", mock_run):
        result = verbose_ctx.run(
            *args,
            stream_output=False,
            capture_output=True,
            timeout_secs=30.0,
            no_output_timeout_secs=60.0,
        )
        assert result == command_result

    # We assert separately because rich will colorize the output
    captured = capfd.readouterr()
    assert "Running" in captured.err
    assert "test command" in captured.err


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
