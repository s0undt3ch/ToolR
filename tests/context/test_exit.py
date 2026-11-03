"""Tests for Context exit functionality."""

from __future__ import annotations

import pytest

from toolr._context import status_to_exit_code


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


@pytest.mark.parametrize(
    ("status", "expected"),
    [(0, 0), (3, 3), (255, 255), (-15, 143), (-9, 137), (256, 1), (-1000, 1), (-1073741819, 1)],
)
def test_status_to_exit_code(status, expected):
    assert status_to_exit_code(status) == expected


def test_exit_maps_negative_status_to_128_plus_signal(ctx, capfd):
    with pytest.raises(SystemExit) as exc_info:
        ctx.exit(-15, "killed")
    assert exc_info.value.code == 143
    assert "killed" in capfd.readouterr().err
