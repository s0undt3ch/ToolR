"""Tests for core Context functionality."""

from __future__ import annotations

import os
import pathlib
import shutil
from unittest.mock import Mock

import pytest

from toolr._context import Context
from toolr.utils._console import Consoles
from toolr.utils._console import ConsoleVerbosity
from toolr.utils.command import CommandResult


def test_context_frozen(ctx):
    """Test that Context is frozen."""
    with pytest.raises(AttributeError) as excinfo:
        ctx._console_stderr = None
    assert "immutable type: 'Context'" in str(excinfo.value)


def test_run_basic(parser, repo_root):
    """Test basic command execution."""
    args = ("echo", "hello")
    command_result = CommandResult(args=args, stdout="output", stderr="", returncode=0)
    run_impl = Mock(return_value=command_result)
    verbosity = ConsoleVerbosity.NORMAL
    consoles = Consoles.setup_no_colors(verbosity)
    ctx = Context(
        parser=parser,
        repo_root=repo_root,
        verbosity=verbosity,
        _console_stderr=consoles.stderr,
        _console_stdout=consoles.stdout,
        _run_impl=run_impl,
    )

    result = ctx.run(*args)
    run_impl.assert_called_once_with(
        ("echo", "hello"),
        stream_output=True,
        capture_output=False,
        timeout_secs=None,
        no_output_timeout_secs=None,
    )
    assert result.stdout == "output"
    assert result.returncode == 0


def test_run_with_options(parser, repo_root):
    """Test command execution with various options."""
    args = ("ls", "-l")
    command_result = CommandResult(args=args, stdout="", stderr="", returncode=0)
    run_impl = Mock(return_value=command_result)
    verbosity = ConsoleVerbosity.NORMAL
    consoles = Consoles.setup_no_colors(verbosity)
    ctx = Context(
        parser=parser,
        repo_root=repo_root,
        verbosity=verbosity,
        _console_stderr=consoles.stderr,
        _console_stdout=consoles.stdout,
        _run_impl=run_impl,
    )

    ctx.run(
        *args,
        stream_output=False,
        capture_output=True,
        timeout_secs=10,
        no_output_timeout_secs=5,
        custom_kwarg="value",
    )
    run_impl.assert_called_once_with(
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
