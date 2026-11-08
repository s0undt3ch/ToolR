"""Tests for Context exit functionality."""

from __future__ import annotations

import pytest

from toolr._context import status_to_exit_code

unix_only = pytest.mark.skip_on_windows(reason="POSIX signals")
windows_only = pytest.mark.skip_unless_on_windows(reason="Windows has no signal deaths")


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
    [
        pytest.param(0, 0, id="success"),
        pytest.param(3, 3, id="failure"),
        pytest.param(255, 255, id="byte-max"),
        pytest.param(True, 1, id="bool"),
        pytest.param(256, 1, id="above-byte"),
        pytest.param(-1073741819, 1, id="windows-crash-status"),
        pytest.param(-1000, 1, id="negative-past-byte"),
        pytest.param(-15, 143, id="sigterm", marks=unix_only),
        pytest.param(-9, 137, id="sigkill", marks=unix_only),
        pytest.param(-1, 129, id="sighup", marks=unix_only),
        pytest.param(-15, 1, id="negative-is-out-of-range", marks=windows_only),
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


@unix_only
def test_exit_maps_negative_status_to_128_plus_signal(ctx, capfd):
    with pytest.raises(SystemExit) as exc_info:
        ctx.exit(-15, "killed")
    assert exc_info.value.code == 143
    assert "killed" in capfd.readouterr().err
