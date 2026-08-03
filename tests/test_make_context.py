"""Tests for `toolr.testing.make_context`."""

from __future__ import annotations

from pathlib import Path

import pytest

from toolr.testing import make_context


def test_make_context_sets_repo_root(tmp_path: Path) -> None:
    result = make_context(tmp_path)
    assert result.ctx.repo_root == tmp_path


def test_make_context_captures_stdout(tmp_path: Path) -> None:
    result = make_context(tmp_path)
    result.ctx.print("hello, world")
    assert "hello, world" in result.output.stdout


def test_make_context_captures_stderr(tmp_path: Path) -> None:
    result = make_context(tmp_path)
    result.ctx.error("something broke")
    assert "something broke" in result.output.stderr


def test_make_context_exit_raises_system_exit(tmp_path: Path) -> None:
    result = make_context(tmp_path)
    with pytest.raises(SystemExit) as exc_info:
        result.ctx.exit(2, "bad input")
    assert exc_info.value.code == 2
