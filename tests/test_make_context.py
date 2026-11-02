"""Tests for `toolr.testing.make_context`."""

from __future__ import annotations

import io
from pathlib import Path

import pytest

from toolr._context import Context
from toolr.testing import ConsoleVerbosity
from toolr.testing import RunMock
from toolr.testing import make_command_result
from toolr.testing import make_context
from toolr.utils import _console
from toolr.utils import command

# The `run=`/`chdir=` doc-snippet tests below also import `RunMock` and `Mock` locally, so the
# `--8<--` snippets they carry are self-contained when included in docs/writing-commands/testing.md.


def test_make_context_sets_repo_root(tmp_path: Path) -> None:
    ctx = make_context(tmp_path)
    assert ctx.repo_root == tmp_path


def test_make_context_captures_stdout(tmp_path: Path) -> None:
    ctx = make_context(tmp_path)
    ctx.print("hello, world")
    assert "hello, world" in ctx.stdout


def test_make_context_captures_stderr(tmp_path: Path) -> None:
    ctx = make_context(tmp_path)
    ctx.error("something broke")
    assert "something broke" in ctx.stderr


def test_console_verbosity_is_exported_from_toolr_testing() -> None:
    assert ConsoleVerbosity is _console.ConsoleVerbosity


@pytest.mark.parametrize(
    ("verbosity", "debug_shown", "info_shown"),
    [
        (ConsoleVerbosity.QUIET, False, False),
        (ConsoleVerbosity.NORMAL, False, True),
        (ConsoleVerbosity.VERBOSE, True, True),
    ],
)
# Keep `ContextForTesting.stdout`/`.stderr` and `ConsoleVerbosity` docstrings in step with this test.
def test_make_context_stream_split(
    tmp_path: Path, verbosity: ConsoleVerbosity, debug_shown: bool, info_shown: bool
) -> None:
    run_mock = RunMock()
    run_mock.mock.return_value = make_command_result()
    ctx = make_context(tmp_path, prompt_input="bob\n", run=run_mock, verbosity=verbosity)
    ctx.print("to-stdout")
    ctx.debug("debug-line")
    ctx.info("info-line")
    ctx.warn("warn-line")
    ctx.error("error-line")
    ctx.prompt("Name?")
    ctx.run("echo", "sub")
    with pytest.raises(SystemExit):
        ctx.exit(1, "exit-message")
    assert "to-stdout" in ctx.stdout
    assert "Name?: " in ctx.stdout
    assert "Name?" not in ctx.stderr
    for text in ("warn-line", "error-line", "exit-message"):
        assert text in ctx.stderr
    assert ("debug-line" in ctx.stderr) is debug_shown
    assert ("info-line" in ctx.stderr) is info_shown
    assert ("Running 'echo sub'" in ctx.stderr) is info_shown
    for text in (
        "debug-line",
        "info-line",
        "warn-line",
        "error-line",
        "Running 'echo sub'",
        "exit-message",
    ):
        assert text not in ctx.stdout


def test_make_context_exit_ok_message_goes_to_stderr(tmp_path: Path) -> None:
    ctx = make_context(tmp_path)
    with pytest.raises(SystemExit) as exc_info:
        ctx.exit(0, "all done")
    assert exc_info.value.code == 0
    assert "all done" in ctx.stderr
    assert "all done" not in ctx.stdout


def test_make_context_prompt_renders_choices_and_reasks_to_stdout(tmp_path: Path) -> None:
    ctx = make_context(tmp_path, prompt_input="x\n3\n2\n")
    assert ctx.prompt("Pick", int, choices=["1", "2"], default=2) == 2
    assert ctx.stdout == (
        "Pick [1/2] (2): Please enter a valid integer number\n"
        "Pick [1/2] (2): Please select one of the available options\n"
        "Pick [1/2] (2): "
    )
    assert ctx.stderr == ""


def test_make_context_exit_raises_system_exit(tmp_path: Path) -> None:
    ctx = make_context(tmp_path)
    with pytest.raises(SystemExit) as exc_info:
        ctx.exit(2, "bad input")
    assert exc_info.value.code == 2


def test_make_context_run_param_wires_into_ctx_run(tmp_path):
    # --8<-- [start:mock-run-example]
    from toolr.testing import RunMock
    from toolr.testing import make_command_result

    run_mock = RunMock()
    run_mock.mock.return_value = make_command_result(stdout="hi\n")

    ctx = make_context(tmp_path, run=run_mock)
    output = ctx.run("echo", "hi", capture_output=True)

    assert output.stdout.read() == "hi\n"
    run_mock.assert_called_once_with(
        ("echo", "hi"),
        stream_output=True,
        capture_output=True,
        timeout_secs=None,
        no_output_timeout_secs=None,
        interactive=False,
    )
    # --8<-- [end:mock-run-example]


def test_make_context_run_param_omitted_uses_real_runner(tmp_path):
    ctx = make_context(tmp_path)

    assert ctx._run_impl is command.run


def test_make_context_chdir_param_wires_into_ctx_chdir(tmp_path):
    # --8<-- [start:mock-chdir-example]
    from unittest.mock import Mock

    chdir_mock = Mock()
    ctx = make_context(tmp_path, chdir=chdir_mock)
    target = tmp_path / "somewhere"
    target.mkdir()

    with ctx.chdir(target):
        pass

    chdir_mock.assert_any_call(target)
    assert chdir_mock.call_count == 2
    # --8<-- [end:mock-chdir-example]


def test_make_context_prompt_input_param_feeds_a_canned_answer(tmp_path):
    # --8<-- [start:mock-prompt-example]
    ctx = make_context(tmp_path, prompt_input="y\n")

    assert ctx.prompt("Continue?", bool) is True
    # --8<-- [end:mock-prompt-example]


def test_make_context_prompt_input_feeds_an_abort_answer(tmp_path):
    # --8<-- [start:mock-prompt-abort-example]
    ctx = make_context(tmp_path, prompt_input="n\n")

    assert ctx.prompt("Continue?", bool) is False
    # --8<-- [end:mock-prompt-abort-example]


def test_make_context_prompt_input_omitted_defaults_to_none(tmp_path):
    ctx = make_context(tmp_path)

    assert ctx._prompt_stream is None


def test_make_context_prompt_input_accepts_a_textio_directly(tmp_path):
    """A `TextIO` passed directly (not a plain `str`) is used as-is, not wrapped in the
    EOF-on-exhaustion helper — only `str` gets that treatment."""
    stream = io.StringIO("y\n")
    ctx = make_context(tmp_path, prompt_input=stream)

    assert ctx._prompt_stream is stream
    assert ctx.prompt("Continue?", bool) is True


def test_make_context_prompt_input_exhaustion_raises_instead_of_hanging(tmp_path):
    ctx = make_context(tmp_path, prompt_input="")  # no answers provided

    with pytest.raises(EOFError):
        ctx.prompt("Continue?", bool)  # no `default=` — would hang on a plain StringIO


def test_make_context_returns_a_context_subclass(tmp_path):
    ctx = make_context(tmp_path)

    assert isinstance(ctx, Context)
