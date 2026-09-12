"""Tests for Context command execution functionality."""

from __future__ import annotations

import io
from unittest.mock import Mock

from rich.console import Console

from toolr._context import Context
from toolr.utils import command
from toolr.utils._console import TOOLR_THEME
from toolr.utils._console import ConsoleVerbosity
from toolr.utils.command import CommandResult


def test_run_command_basic(parser, repo_root):
    """Test run method with basic command."""
    args = ("echo", "hello")
    command_result = CommandResult(args=args, stdout="output", stderr="", returncode=0)
    run_impl = Mock(return_value=command_result)

    stderr_buffer = io.StringIO()
    console_stderr = Console(
        file=stderr_buffer, stderr=True, force_terminal=False, theme=TOOLR_THEME
    )
    console_stdout = Console(
        file=io.StringIO(), stderr=False, force_terminal=False, theme=TOOLR_THEME
    )

    ctx = Context(
        repo_root=repo_root,
        parser=parser,
        verbosity=ConsoleVerbosity.VERBOSE,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
        _run_impl=run_impl,
    )

    result = ctx.run(*args)
    assert result == command_result

    # We assert separately because rich will colorize the output
    assert "Running" in stderr_buffer.getvalue()
    assert "echo hello" in stderr_buffer.getvalue()


def test_run_command_echo_is_literal_not_markup(parser, repo_root):
    """The 'Running ...' echo prints the cmdline literally, never as rich markup.

    A command argument that looks like a rich tag (``[red]``, ``[link=…]``)
    must appear verbatim — otherwise rich would consume the tag and the echo
    would lie about what actually ran. Guards the ``markup=False`` on the echo.
    """
    args = ("echo", "[red]hi[/red]")
    command_result = CommandResult(args=args, stdout="", stderr="", returncode=0)
    run_impl = Mock(return_value=command_result)

    stderr_buffer = io.StringIO()
    console_stderr = Console(
        file=stderr_buffer, stderr=True, force_terminal=False, theme=TOOLR_THEME
    )
    console_stdout = Console(
        file=io.StringIO(), stderr=False, force_terminal=False, theme=TOOLR_THEME
    )

    ctx = Context(
        repo_root=repo_root,
        parser=parser,
        verbosity=ConsoleVerbosity.VERBOSE,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
        _run_impl=run_impl,
    )

    ctx.run(*args)

    # The literal tag survives (markup not interpreted/stripped).
    assert "[red]hi[/red]" in stderr_buffer.getvalue()


def test_run_command_with_options(parser, repo_root):
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

    stderr_buffer = io.StringIO()
    console_stderr = Console(
        file=stderr_buffer, stderr=True, force_terminal=False, theme=TOOLR_THEME
    )
    console_stdout = Console(
        file=io.StringIO(), stderr=False, force_terminal=False, theme=TOOLR_THEME
    )

    ctx = Context(
        repo_root=repo_root,
        parser=parser,
        verbosity=ConsoleVerbosity.VERBOSE,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
        _run_impl=mock_run,
    )

    result = ctx.run(
        *args,
        stream_output=False,
        capture_output=True,
        timeout_secs=30.0,
        no_output_timeout_secs=60.0,
    )
    assert result == command_result

    # We assert separately because rich will colorize the output
    assert "Running" in stderr_buffer.getvalue()
    assert "test command" in stderr_buffer.getvalue()


def test_run_uses_run_impl_override(parser, repo_root):
    """Test that Context.run uses _run_impl when provided."""
    fake_result = CommandResult(args=["echo", "hi"], stdout=None, stderr=None, returncode=0)
    run_impl = Mock(return_value=fake_result)

    console_stderr = Console(
        file=io.StringIO(), stderr=True, force_terminal=False, theme=TOOLR_THEME
    )
    console_stdout = Console(
        file=io.StringIO(), stderr=False, force_terminal=False, theme=TOOLR_THEME
    )

    ctx = Context(
        repo_root=repo_root,
        parser=parser,
        verbosity=ConsoleVerbosity.NORMAL,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
        _run_impl=run_impl,
    )

    result = ctx.run("echo", "hi")

    assert result is fake_result
    run_impl.assert_called_once_with(
        ("echo", "hi"),
        stream_output=True,
        capture_output=False,
        timeout_secs=None,
        no_output_timeout_secs=None,
        interactive=False,
    )


def test_run_defaults_to_real_command_run(parser, repo_root):
    """Omitting `_run_impl` at construction time keeps today's real-subprocess behavior."""
    console_stderr = Console(
        file=io.StringIO(), stderr=True, force_terminal=False, theme=TOOLR_THEME
    )
    console_stdout = Console(
        file=io.StringIO(), stderr=False, force_terminal=False, theme=TOOLR_THEME
    )

    ctx = Context(
        repo_root=repo_root,
        parser=parser,
        verbosity=ConsoleVerbosity.NORMAL,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
    )

    assert ctx._run_impl is command.run


def test_run_interactive_defaults_stream_output_to_false(parser, repo_root):
    """interactive=True must not trip the stream_output incompatibility by default (#485)."""
    fake_result = CommandResult(args=["echo", "hi"], stdout=None, stderr=None, returncode=0)
    run_impl = Mock(return_value=fake_result)

    console_stderr = Console(
        file=io.StringIO(), stderr=True, force_terminal=False, theme=TOOLR_THEME
    )
    console_stdout = Console(
        file=io.StringIO(), stderr=False, force_terminal=False, theme=TOOLR_THEME
    )

    ctx = Context(
        repo_root=repo_root,
        parser=parser,
        verbosity=ConsoleVerbosity.NORMAL,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
        _run_impl=run_impl,
    )

    ctx.run("echo", "hi", interactive=True)

    run_impl.assert_called_once_with(
        ("echo", "hi"),
        stream_output=False,
        capture_output=False,
        timeout_secs=None,
        no_output_timeout_secs=None,
        interactive=True,
    )


def test_run_interactive_ignores_default_no_output_timeout_secs(parser, repo_root):
    """A `--no-output-timeout-secs` set on the Context must not leak into
    `interactive=True` calls — the caller didn't ask for that watchdog (#485)."""
    fake_result = CommandResult(args=["echo", "hi"], stdout=None, stderr=None, returncode=0)
    run_impl = Mock(return_value=fake_result)

    console_stderr = Console(
        file=io.StringIO(), stderr=True, force_terminal=False, theme=TOOLR_THEME
    )
    console_stdout = Console(
        file=io.StringIO(), stderr=False, force_terminal=False, theme=TOOLR_THEME
    )

    ctx = Context(
        repo_root=repo_root,
        parser=parser,
        verbosity=ConsoleVerbosity.NORMAL,
        _console_stderr=console_stderr,
        _console_stdout=console_stdout,
        _run_impl=run_impl,
        default_no_output_timeout_secs=30.0,
    )

    ctx.run("echo", "hi", interactive=True)

    run_impl.assert_called_once_with(
        ("echo", "hi"),
        stream_output=False,
        capture_output=False,
        timeout_secs=None,
        no_output_timeout_secs=None,
        interactive=True,
    )
