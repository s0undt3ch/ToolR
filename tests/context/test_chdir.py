"""Tests for `Context.chdir`, including the `_chdir_impl` injection seam."""

from __future__ import annotations

import pathlib
from unittest.mock import Mock


def test_chdir_uses_real_os_chdir_by_default(ctx, tmp_path):
    """The default path still really moves the process cwd (and restores it)."""
    target = tmp_path / "somewhere"
    target.mkdir()
    original_cwd = pathlib.Path.cwd()

    with ctx.chdir(target) as p:
        assert p == target
        assert pathlib.Path.cwd() == target

    assert pathlib.Path.cwd() == original_cwd


def test_chdir_uses_chdir_impl_override_without_touching_real_cwd(ctx, tmp_path):
    """Mocking _chdir_impl prevents real cwd changes."""
    chdir_impl = Mock()
    ctx = ctx.replace(_chdir_impl=chdir_impl)
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
