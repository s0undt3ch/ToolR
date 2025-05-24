"""Tests for the Context class."""

from __future__ import annotations

import os
import pathlib
import shutil
from argparse import ArgumentParser
from dataclasses import FrozenInstanceError
from unittest import mock

import pytest

from toolr.context import ConsoleVerbosity
from toolr.context import Context
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
    return Context(parser=parser, repo_root=repo_root)


def test_context_frozen(ctx):
    """Test that Context is a frozen dataclass."""
    with pytest.raises(FrozenInstanceError):
        ctx.console = None


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
    test_dir = tmp_path / "test_dir"
    test_dir.mkdir()

    assert pathlib.Path.cwd() == temp_cwd
    with ctx.chdir(test_dir) as path:
        assert pathlib.Path.cwd() == test_dir == path
    assert pathlib.Path.cwd() == temp_cwd


def test_chdir_nonexistent_original(ctx, temp_cwd, tmp_path):
    """Test chdir when original directory no longer exists."""
    test_dir = tmp_path / "test_dir"
    test_dir.mkdir()

    assert pathlib.Path.cwd() == temp_cwd
    with mock.patch.object(ctx.console, "log") as mock_log:
        with ctx.chdir(test_dir) as path:
            assert pathlib.Path.cwd() == test_dir == path
            shutil.rmtree(temp_cwd)
        mock_log.assert_called_with(f"Unable to change back to path {temp_cwd}", style="log-error", _stack_offset=2)


def test_chdir_str_path(ctx):
    """Test chdir with string path."""
    path_str = "/test/dir"
    test_dir = pathlib.Path(path_str).resolve()

    with (
        mock.patch("os.chdir") as mock_chdir,
        mock.patch("pathlib.Path.resolve", return_value=test_dir),
        mock.patch("pathlib.Path.exists", return_value=True),
    ):
        with ctx.chdir(path_str) as path:
            assert path == test_dir
            mock_chdir.assert_called_with(test_dir)


def test_debug_output(parser, repo_root):
    """Test debug output during command execution."""
    ctx = Context(parser=parser, repo_root=repo_root, verbosity=ConsoleVerbosity.VERBOSE)

    with mock.patch("toolr.utils.command.run") as mock_run, mock.patch.object(ctx.console, "log") as mock_log:
        args = ("echo", "hello")
        mock_run.return_value = CommandResult(args=args, stdout="", stderr="", returncode=0)
        ctx.run(*args)
        mock_log.assert_called_once_with("Running 'echo hello'", style="log-debug", _stack_offset=2)
