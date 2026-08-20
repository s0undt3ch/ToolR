"""Canonical mock for `Context.run`, for use with `toolr.testing.make_context(run=...)`."""

from __future__ import annotations

import io
from collections.abc import Sequence
from typing import Any
from unittest.mock import Mock

import msgspec

from toolr.utils.command import CommandResult


def make_command_result(
    *,
    args: list[str] | None = None,
    stdout: str | bytes = "",
    stderr: str | bytes = "",
    returncode: int = 0,
) -> CommandResult[str] | CommandResult[bytes]:
    """Build a genuine `CommandResult`, with fresh `stdout`/`stderr` streams."""
    if isinstance(stdout, bytes) or isinstance(stderr, bytes):
        # `CommandResult[T]` ties `stdout`/`stderr` to the *same* T — mixing
        # a `str` stdout with a `bytes` stderr (or vice versa) isn't a valid
        # `CommandResult` for any single T, so both must agree; coerce a
        # str default ("") to bytes rather than force both call sites to
        # always pass matching types explicitly.
        stdout_bytes = stdout if isinstance(stdout, bytes) else stdout.encode()
        stderr_bytes = stderr if isinstance(stderr, bytes) else stderr.encode()
        return CommandResult(
            args=args or [],
            stdout=io.BytesIO(stdout_bytes),
            stderr=io.BytesIO(stderr_bytes),
            returncode=returncode,
        )
    return CommandResult(
        args=args or [],
        stdout=io.StringIO(stdout),
        stderr=io.StringIO(stderr),
        returncode=returncode,
    )


class RunMock:
    """Drop-in for `Context._run_impl`. Wraps a `Mock`, not a `MagicMock` subclass.

    Deliberately wraps a `Mock` rather than subclassing it: forwards a
    fixed, explicit subset of its API — `assert_called`, `assert_called_with`,
    `assert_any_call`, `assert_called_once`, `assert_called_once_with`,
    `assert_has_calls`, `assert_not_called`, `call_args`, `call_args_list`,
    `call_count`, `called`, `reset_mock` — rather than a blanket passthrough.
    `Mock`'s own `__getattr__` only guards names that *look like* a mistyped
    assertion (a plain `Mock` already raises `AttributeError` for those, no
    subclassing needed) — every other attribute silently auto-vivifies a fresh
    child `Mock` on a `Mock`/`MagicMock` subclass instead of raising, so a
    typo'd non-assertion call or configuration (e.g. a stray extra letter in
    `call_count` or `reset_mock`) just never runs. Forwarding explicitly
    closes that gap: any name not in the list above is a genuine
    `AttributeError`, typo or not. Reach through to `.mock` directly for
    anything not in that list — `side_effect`/`return_value` are configured
    that way on purpose, so setting them on `run_mock` itself doesn't work
    by accident either.

    `test_run_mock_forwards_every_assertion_method` in
    `tests/testing/test_run_mock.py` diffs this list against
    `dir(Mock())` on every run — adding an `assert_*`/`call_*` method to a
    future Python's `Mock` fails that test until it's forwarded here (or
    added to the test's documented exclusion list), instead of only
    surfacing the next time someone happens to need it.

    Configure it like any other `Mock` — set `.mock.return_value` or
    `.mock.side_effect` to a `CommandResult` (build one with
    `make_command_result(...)`), or a callable/exception for `side_effect`.

    `.mock.return_value` hands back the *same* `CommandResult` on every
    call, and its `stdout`/`stderr` streams are read-once — a command
    under test that calls `ctx.run` more than once gets an empty stream
    on the second and later calls. Fine for a single-call test; for
    anything that calls `ctx.run` repeatedly, use `.mock.side_effect`
    instead — either a callable that builds a fresh `make_command_result(...)`
    per call (optionally dispatching on the `cmdline` argument), or a list
    of pre-built results for `Mock`'s own per-call-consumption behavior.

    Does not inspect or validate the real runner's `text=`/`stream_output=`
    kwargs — whatever `CommandResult` you configure comes back as-is (modulo
    the `capture_output` guard below). A test that exercises those specific
    combinations should assert on them itself rather than relying on `RunMock`
    to reject an invalid combination the way the real runner does.
    """

    def __init__(self) -> None:
        self.mock = Mock(name="RunMock")

    def __call__(
        self, cmdline: tuple[str, ...], **kwargs: object
    ) -> CommandResult[str] | CommandResult[bytes]:
        # Records the call and resolves `side_effect`/`return_value` in the
        # same step — no need to call `self.mock` a second time to get "the"
        # resolved value; this *is* it. Matches real `Mock` semantics: the
        # call is recorded even if resolving it raises.
        result = self.mock(cmdline, **kwargs)

        # Nothing configured — `result` is just a plain, auto-vivified
        # `Mock`, not a `CommandResult`. Checking the actual return type
        # (rather than trying to detect "was return_value explicitly set",
        # which has no reliable sentinel) is what lets us raise a clear,
        # actionable error here instead of letting `msgspec.structs.replace`
        # below fail with a confusing, off-topic `TypeError`.
        if not isinstance(result, CommandResult):
            msg = (
                "RunMock: `run_mock.mock` has no `side_effect`/`return_value` that returns a "
                "`CommandResult`. Set `run_mock.mock.side_effect` or "
                "`run_mock.mock.return_value` to a `CommandResult` built with "
                "`make_command_result(...)`."
            )
            raise TypeError(msg)

        # Applies regardless of how `result` was produced: the real runner
        # has no way to "opt out" of this either — it only ever captures
        # output when asked. A manually-built `CommandResult` with populated
        # `stdout`/`stderr` still comes back with both forced to `None` if
        # the call itself didn't request `capture_output=True`.
        if not kwargs.get("capture_output"):
            result = msgspec.structs.replace(result, stdout=None, stderr=None)
        return result

    # Explicit, fixed forwarding to the underlying Mock — not a blanket
    # __getattr__, so a genuine typo raises AttributeError instead of
    # silently returning a fresh child Mock.
    def assert_called(self) -> None:
        self.mock.assert_called()

    def assert_called_with(self, *args: object, **kwargs: object) -> None:
        self.mock.assert_called_with(*args, **kwargs)

    def assert_any_call(self, *args: object, **kwargs: object) -> None:
        self.mock.assert_any_call(*args, **kwargs)

    def assert_called_once(self) -> None:
        self.mock.assert_called_once()

    def assert_called_once_with(self, *args: object, **kwargs: object) -> None:
        self.mock.assert_called_once_with(*args, **kwargs)

    def assert_has_calls(self, calls: Sequence[Any], any_order: bool = False) -> None:
        self.mock.assert_has_calls(calls, any_order=any_order)

    def assert_not_called(self) -> None:
        self.mock.assert_not_called()

    @property
    def call_args(self) -> Any:  # mirrors unittest.mock's own untyped property
        return self.mock.call_args

    @property
    def call_args_list(self) -> Any:
        return self.mock.call_args_list

    @property
    def call_count(self) -> int:
        return self.mock.call_count

    @property
    def called(self) -> bool:
        return self.mock.called

    def reset_mock(self, *, return_value: bool = False, side_effect: bool = False) -> None:
        """Clear the call log.

        Forwarded straight to `Mock.reset_mock`, so — matching that
        method's own default — a `side_effect`/`return_value` configured
        directly on `.mock` is *not* cleared unless you pass
        `side_effect=True`/`return_value=True`, same as a plain `Mock`.
        """
        self.mock.reset_mock(return_value=return_value, side_effect=side_effect)
