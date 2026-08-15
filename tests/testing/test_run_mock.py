"""Unit tests for `toolr.testing.RunMock` and `make_command_result`."""

from __future__ import annotations

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


def test_run_mock_result_args_reflects_actual_invocation_not_registered_prefix():
    run_mock = RunMock()
    run_mock.register("git", stdout="ok\n")

    result = run_mock(("git", "fetch", "--all"), capture_output=True)

    assert result.args == ["git", "fetch", "--all"]


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


def test_run_mock_equal_length_prefixes_resolve_in_registration_order():
    """Among equal-length prefixes, the first-registered one wins while it has
    occurrences remaining — registering a second same-prefix entry later does
    NOT override it; it only takes over once the first is exhausted."""
    run_mock = RunMock()
    run_mock.register("git", "status", stdout="first-registered\n")
    run_mock.register("git", "status", stdout="second-registered\n")

    first = run_mock(("git", "status"), capture_output=True)
    second = run_mock(("git", "status"), capture_output=True)

    assert first.stdout.read() == "first-registered\n"
    assert second.stdout.read() == "first-registered\n"


def test_run_mock_reset_mock_clears_registrations():
    """After `reset_mock()`, an unregistered call falls through to the plain
    `Mock` escape hatch instead of resolving against a stale registration —
    and since nothing is configured there either, it raises the same clear
    "unconfigured" error as a `RunMock` that was never registered at all."""
    run_mock = RunMock()
    run_mock.register("git", "fetch", stdout="up to date\n")

    run_mock(("git", "fetch"), capture_output=True)
    run_mock.reset_mock()

    assert run_mock.call_count == 0
    with pytest.raises(TypeError, match="register"):
        run_mock(("git", "fetch"), capture_output=True)


def test_run_mock_register_raises_when_side_effect_already_set():
    run_mock = RunMock()
    run_mock.mock.side_effect = RuntimeError("boom")

    with pytest.raises(TypeError, match="side_effect"):
        run_mock.register("git", "fetch")


def test_run_mock_raises_clear_error_when_unconfigured():
    run_mock = RunMock()

    with pytest.raises(TypeError, match="register"):
        run_mock(("git", "fetch"), capture_output=True)


def test_run_mock_escape_hatch_return_value_is_honored():
    """A manually-configured `return_value` returning a real `CommandResult`
    is a valid escape hatch and must not trip the "unconfigured" guard."""
    run_mock = RunMock()
    run_mock.mock.return_value = make_command_result(stdout="from return_value\n")

    result = run_mock(("anything",), capture_output=True)

    assert result.stdout.read() == "from return_value\n"
