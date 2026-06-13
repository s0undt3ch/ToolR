"""Tests for Context command execution functionality."""

from __future__ import annotations

from unittest import mock

from toolr.utils.command import CommandResult


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


def test_run_command_echo_is_literal_not_markup(verbose_ctx, capfd):
    """The 'Running ...' echo prints the cmdline literally, never as rich markup.

    A command argument that looks like a rich tag (``[red]``, ``[link=…]``)
    must appear verbatim — otherwise rich would consume the tag and the echo
    would lie about what actually ran. Guards the ``markup=False`` on the echo.
    """
    args = ("echo", "[red]hi[/red]")
    command_result = CommandResult(args=args, stdout="", stderr="", returncode=0)
    with mock.patch("toolr.utils.command.run", return_value=command_result):
        verbose_ctx.run(*args)

    captured = capfd.readouterr()
    # The literal tag survives (markup not interpreted/stripped).
    assert "[red]hi[/red]" in captured.err


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
