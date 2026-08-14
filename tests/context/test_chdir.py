"""Tests for `Context.chdir`, including the `_chdir_impl` injection seam."""

from __future__ import annotations

import pathlib
from unittest.mock import Mock

from toolr._context import Context
from toolr.utils._console import Consoles
from toolr.utils._console import ConsoleVerbosity


def test_chdir_uses_real_os_chdir_by_default(parser, repo_root, tmp_path):
    """The default path still really moves the process cwd (and restores it)."""
    verbosity = ConsoleVerbosity.NORMAL
    consoles = Consoles.setup_no_colors(verbosity)
    ctx = Context(
        repo_root=repo_root,
        parser=parser,
        verbosity=verbosity,
        _console_stderr=consoles.stderr,
        _console_stdout=consoles.stdout,
    )
    target = tmp_path / "somewhere"
    target.mkdir()
    original_cwd = pathlib.Path.cwd()

    with ctx.chdir(target) as p:
        assert p == target
        assert pathlib.Path.cwd() == target

    assert pathlib.Path.cwd() == original_cwd


def test_chdir_uses_chdir_impl_override_without_touching_real_cwd(parser, repo_root, tmp_path):
    """Mocking _chdir_impl prevents real cwd changes."""
    chdir_impl = Mock()
    verbosity = ConsoleVerbosity.NORMAL
    consoles = Consoles.setup_no_colors(verbosity)
    ctx = Context(
        repo_root=repo_root,
        parser=parser,
        verbosity=verbosity,
        _console_stderr=consoles.stderr,
        _console_stdout=consoles.stdout,
        _chdir_impl=chdir_impl,
    )
    target = tmp_path / "somewhere"
    original_cwd = pathlib.Path.cwd()

    with ctx.chdir(target):
        pass

    # Called twice: once to enter, once to restore.
    chdir_impl.assert_any_call(target)
    chdir_impl.assert_any_call(original_cwd)
    assert chdir_impl.call_count == 2
    # The real process cwd never moved.
    assert pathlib.Path.cwd() == original_cwd
