"""Tests for Context exit functionality."""

from __future__ import annotations

import pytest


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
