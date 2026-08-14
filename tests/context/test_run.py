"""Tests for Context command execution functionality."""

from __future__ import annotations

from unittest import mock
from unittest.mock import Mock

from toolr.testing import make_context
from toolr.utils import command
from toolr.utils.command import CommandResult


def test_run_command_basic(tmp_path):
    """Test run method with basic command."""
    args = ("echo", "hello")
    command_result = CommandResult(args=args, stdout="output", stderr="", returncode=0)
    result_ctx = make_context(tmp_path, run=Mock(return_value=command_result))
    result = result_ctx.ctx.run(*args)
    assert result == command_result

    # We assert separately because rich will colorize the output
    assert "Running" in result_ctx.output.stderr
    assert "echo hello" in result_ctx.output.stderr


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


def test_run_command_with_options(tmp_path):
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

    result_ctx = make_context(tmp_path, run=mock_run)
    result = result_ctx.ctx.run(
        *args,
        stream_output=False,
        capture_output=True,
        timeout_secs=30.0,
        no_output_timeout_secs=60.0,
    )
    assert result == command_result

    # We assert separately because rich will colorize the output
    assert "Running" in result_ctx.output.stderr
    assert "test command" in result_ctx.output.stderr


def test_run_uses_run_impl_override(tmp_path):
    """Test that Context.run uses _run_impl when provided."""
    fake_result = CommandResult(args=["echo", "hi"], stdout=None, stderr=None, returncode=0)
    run_impl = Mock(return_value=fake_result)

    result_ctx = make_context(tmp_path, run=run_impl)
    ctx = result_ctx.ctx
    result = ctx.run("echo", "hi")

    assert result is fake_result
    run_impl.assert_called_once_with(
        ("echo", "hi"),
        stream_output=True,
        capture_output=False,
        timeout_secs=None,
        no_output_timeout_secs=None,
    )


def test_run_defaults_to_real_command_run(tmp_path):
    """Omitting `_run_impl` at construction time keeps today's real-subprocess behavior."""
    result_ctx = make_context(tmp_path)
    ctx = result_ctx.ctx

    assert ctx._run_impl is command.run
