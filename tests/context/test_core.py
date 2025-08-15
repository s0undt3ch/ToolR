"""Tests for core Context functionality."""

from __future__ import annotations

import os
import pathlib
import shutil
from unittest import mock

import pytest

from toolr.utils.command import CommandResult


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
