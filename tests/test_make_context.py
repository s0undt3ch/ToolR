"""Tests for `toolr.testing.make_context`."""

from __future__ import annotations

from pathlib import Path

import pytest

from toolr._context import Context
from toolr.testing import make_context
from toolr.utils import command

# `RunMock` and `Mock` are imported locally inside the `run=`/`chdir=` tests below rather than
# here, so the `--8<--` doc snippets they carry are self-contained when included in
# docs/writing-commands/testing.md.


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


def test_make_context_prompt_input_exhaustion_raises_instead_of_hanging(tmp_path):
    ctx = make_context(tmp_path, prompt_input="")  # no answers provided

    with pytest.raises(EOFError):
        ctx.prompt("Continue?", bool)  # no `default=` — would hang on a plain StringIO


def test_make_context_returns_a_context_subclass(tmp_path):
    ctx = make_context(tmp_path)

    assert isinstance(ctx, Context)
