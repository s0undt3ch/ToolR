"""Tests for Context exit functionality."""

from __future__ import annotations

import sys

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


IS_WINDOWS = sys.platform.startswith("win")


@pytest.mark.parametrize(
    ("status", "expected"),
    [
        (0, 0),
        (3, 3),
        (255, 255),
        (True, 1),
        (256, 1),
        (-1000, 1),
        (-1073741819, 1),
        # A signal death on Unix; a real, out-of-range exit code on Windows.
        (-15, 1 if IS_WINDOWS else 143),
        (-9, 1 if IS_WINDOWS else 137),
        (-1, 1 if IS_WINDOWS else 129),
    ],
)
def test_status_to_exit_code(status, expected):
    result = status_to_exit_code(status)
    assert result == expected
    assert type(result) is int


@pytest.mark.parametrize(("status", "expected"), [(None, None), ("boom", "boom")])
def test_exit_passes_non_int_status_to_sys_exit_unchanged(ctx, status, expected):
    with pytest.raises(SystemExit) as exc_info:
        ctx.exit(status)
    assert exc_info.value.code == expected


def test_exit_maps_negative_status_to_128_plus_signal(ctx, capfd):
    with pytest.raises(SystemExit) as exc_info:
        ctx.exit(-15, "killed")
    assert exc_info.value.code == (1 if IS_WINDOWS else 143)
    assert "killed" in capfd.readouterr().err
