"""Unit tests for `toolr.testing.RunMock` and `make_command_result`."""

from __future__ import annotations

import io

import pytest

from toolr.testing._run_mock import RunMock
from toolr.testing._run_mock import make_command_result


def test_make_command_result_builds_str_streams():
    result = make_command_result(stdout="out", stderr="err", returncode=0)
    assert result.stdout.read() == "out"
    assert result.stderr.read() == "err"
    assert result.returncode == 0


def test_make_command_result_builds_bytes_streams():
    result = make_command_result(stdout=b"out", stderr=b"err")
    assert result.stdout.read() == b"out"
    assert result.stderr.read() == b"err"


def test_make_command_result_mints_fresh_streams_each_call():
    """Two calls with the same args must not share one exhausted stream."""
    first = make_command_result(stdout="out")
    second = make_command_result(stdout="out")
    assert first.stdout.read() == "out"
    assert second.stdout.read() == "out"  # not "" — it's a fresh stream


def test_run_mock_records_calls():
    run_mock = RunMock()
    run_mock.register("git", "fetch", stdout="up to date\n")

    run_mock(("git", "fetch"), stream_output=True, capture_output=True)

    run_mock.assert_called_once_with(("git", "fetch"), stream_output=True, capture_output=True)
    assert run_mock.call_count == 1


def test_run_mock_resolves_registration_by_longest_prefix():
    run_mock = RunMock()
    run_mock.register("git", stdout="generic git output\n")
    run_mock.register("git", "fetch", stdout="fetch-specific output\n")

    result = run_mock(("git", "fetch"), capture_output=True)

    assert result.stdout.read() == "fetch-specific output\n"


def test_run_mock_forces_stdout_and_stderr_none_without_capture_output():
    run_mock = RunMock()
    run_mock.register("git", "fetch", stdout="up to date\n")

    result = run_mock(("git", "fetch"), capture_output=False)

    assert result.stdout is None
    assert result.stderr is None


def test_run_mock_raises_on_unregistered_call():
    run_mock = RunMock()
    run_mock.register("git", "fetch", stdout="up to date\n")

    with pytest.raises(AssertionError, match=r"git.*push"):
        run_mock(("git", "push"), capture_output=True)


def test_run_mock_occurrences_exhausts_then_raises():
    run_mock = RunMock()
    run_mock.register("git", "fetch", stdout="first\n", occurrences=1)
    run_mock.register("git", "fetch", stdout="second\n")

    first = run_mock(("git", "fetch"), capture_output=True)
    second = run_mock(("git", "fetch"), capture_output=True)

    assert first.stdout.read() == "first\n"
    assert second.stdout.read() == "second\n"


def test_run_mock_reset_mock_clears_registrations():
    """After `reset_mock()`, an unregistered call falls through to the plain
    `Mock` escape hatch instead of resolving against a stale registration."""
    run_mock = RunMock()
    run_mock.register("git", "fetch", stdout="up to date\n")

    run_mock(("git", "fetch"), capture_output=True)
    run_mock.reset_mock()

    assert run_mock.call_count == 0
    result = run_mock(("git", "fetch"), capture_output=True)
    assert result is not None
    assert not isinstance(result.stdout, io.StringIO)
