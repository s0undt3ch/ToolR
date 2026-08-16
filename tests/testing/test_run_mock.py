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
    run_mock.mock.return_value = make_command_result(stdout="up to date\n")

    run_mock(("git", "fetch"), stream_output=True, capture_output=True)

    run_mock.assert_called_once_with(("git", "fetch"), stream_output=True, capture_output=True)
    assert run_mock.call_count == 1


def test_run_mock_forces_stdout_and_stderr_none_without_capture_output():
    run_mock = RunMock()
    run_mock.mock.return_value = make_command_result(stdout="up to date\n")

    result = run_mock(("git", "fetch"), capture_output=False)

    assert result.stdout is None
    assert result.stderr is None


def test_run_mock_raises_clear_error_when_unconfigured():
    run_mock = RunMock()

    with pytest.raises(TypeError, match="side_effect"):
        run_mock(("git", "push"), capture_output=True)


def test_run_mock_return_value_is_honored():
    """A configured `return_value` returning a real `CommandResult` works
    exactly like a standard `Mock`."""
    run_mock = RunMock()
    run_mock.mock.return_value = make_command_result(stdout="from return_value\n")

    result = run_mock(("anything",), capture_output=True)

    assert result.stdout.read() == "from return_value\n"


def test_run_mock_return_value_still_nulls_output_without_capture_output():
    """The `capture_output` guard applies regardless of how the result was
    configured — a manually-built `CommandResult` with populated
    stdout/stderr still comes back with both forced to `None` if the call
    itself didn't ask for `capture_output=True`, matching real `command.run`."""
    run_mock = RunMock()
    run_mock.mock.return_value = make_command_result(stdout="from return_value\n")

    result = run_mock(("anything",))

    assert result.stdout is None
    assert result.stderr is None


def test_run_mock_return_value_stream_is_exhausted_on_second_call():
    """`return_value` hands back the *same* `CommandResult` every call, so a
    command that calls `ctx.run` more than once gets an empty stream on the
    second and later calls — this is a real footgun to document, not a bug
    to fix: it's how `Mock.return_value` has always worked. `side_effect` is
    the safe choice for multi-call tests (see the next test)."""
    run_mock = RunMock()
    run_mock.mock.return_value = make_command_result(stdout="hi\n")

    first = run_mock(("a",), capture_output=True)
    second = run_mock(("b",), capture_output=True)

    assert first.stdout.read() == "hi\n"
    assert second.stdout.read() == ""  # same object, already exhausted


def test_run_mock_side_effect_dispatches_per_command():
    """A `side_effect` callable is the way to return different results for
    different commands — the standard `unittest.mock` idiom, not a bespoke
    `RunMock` API."""

    def fake_run(cmdline, **_kwargs):
        if cmdline == ("git", "status"):
            return make_command_result(stdout="clean\n")
        return make_command_result(stdout="unknown command\n", returncode=1)

    run_mock = RunMock()
    run_mock.mock.side_effect = fake_run

    status = run_mock(("git", "status"), capture_output=True)
    other = run_mock(("git", "log"), capture_output=True)

    assert status.stdout.read() == "clean\n"
    assert other.stdout.read() == "unknown command\n"
    assert other.returncode == 1


def test_run_mock_side_effect_raising_still_records_the_call():
    """Matches real `Mock` semantics: a `side_effect` that raises still
    counts as a call."""
    run_mock = RunMock()
    run_mock.mock.side_effect = FileNotFoundError("git not found")

    with pytest.raises(FileNotFoundError):
        run_mock(("git", "status"), capture_output=True)

    assert run_mock.call_count == 1


def test_run_mock_assert_not_called_and_called():
    run_mock = RunMock()
    run_mock.mock.return_value = make_command_result(stdout="ok\n")

    assert run_mock.called is False
    run_mock.assert_not_called()

    run_mock(("git", "status"), capture_output=True)

    assert run_mock.called is True
    with pytest.raises(AssertionError):
        run_mock.assert_not_called()


def test_run_mock_reset_mock_clears_call_log():
    run_mock = RunMock()
    run_mock.mock.return_value = make_command_result(stdout="ok\n")

    run_mock(("git", "status"), capture_output=True)
    run_mock.reset_mock()

    assert run_mock.call_count == 0
