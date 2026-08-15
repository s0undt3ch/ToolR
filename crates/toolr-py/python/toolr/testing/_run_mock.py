"""Canonical mock for `Context.run`, for use with `toolr.testing.make_context(run=...)`."""

from __future__ import annotations

import io
from typing import TYPE_CHECKING
from unittest.mock import Mock

import msgspec

from toolr.utils.command import CommandResult

if TYPE_CHECKING:
    from collections.abc import Callable


def make_command_result(
    *,
    args: list[str] | None = None,
    stdout: str | bytes = "",
    stderr: str | bytes = "",
    returncode: int = 0,
) -> CommandResult[str] | CommandResult[bytes]:
    """Build a genuine `CommandResult`, with fresh `stdout`/`stderr` streams."""
    stdout_stream = io.BytesIO(stdout) if isinstance(stdout, bytes) else io.StringIO(stdout)
    stderr_stream = io.BytesIO(stderr) if isinstance(stderr, bytes) else io.StringIO(stderr)
    return CommandResult(
        args=args or [], stdout=stdout_stream, stderr=stderr_stream, returncode=returncode
    )


class _Registration:
    __slots__ = ("make_result", "occurrences", "prefix")

    def __init__(
        self,
        prefix: tuple[str, ...],
        make_result: Callable[[tuple[str, ...]], CommandResult[str] | CommandResult[bytes]],
        occurrences: int | None,
    ) -> None:
        self.prefix = prefix
        self.make_result = make_result
        self.occurrences = occurrences


class RunMock:
    """Drop-in for `Context._run_impl`. Wraps a `Mock`, not a `MagicMock` subclass.

    Standard `Mock` assertions are available via the forwarded methods below.
    `.register(...)` is declarative sugar for the common "these exact args
    return this exact result" case; `side_effect`/`return_value` on the
    underlying `Mock` (accessible as `run_mock.mock`) are the escape hatch
    for anything `.register(...)` can't express.
    """

    def __init__(self) -> None:
        self.mock = Mock(name="RunMock")
        self._registrations: list[_Registration] = []

    def register(
        self,
        *cmdline: str,
        stdout: str | bytes = "",
        stderr: str | bytes = "",
        returncode: int = 0,
        occurrences: int | None = None,
    ) -> None:
        # Only `side_effect` is checked here, not `return_value` — a fresh
        # `Mock()`'s `return_value` is itself a lazily auto-vivified child
        # `Mock`, not a distinguishable "unset" sentinel via the public API,
        # so there's nothing reliable to compare it against. Registrations
        # always take priority when present (see `__call__`); the only real
        # conflict worth guarding against is a `side_effect` that would
        # otherwise silently never run.
        if self.mock.side_effect is not None:
            msg = "cannot mix RunMock.register(...) with a directly-configured side_effect"
            raise TypeError(msg)

        def make_result(
            invoked_cmdline: tuple[str, ...],
        ) -> CommandResult[str] | CommandResult[bytes]:
            # `args` reflects the actual invocation, not the registered
            # prefix — matches the real `command.run`, which sets
            # `args=list(args)` for whatever was actually run.
            return make_command_result(
                args=list(invoked_cmdline), stdout=stdout, stderr=stderr, returncode=returncode
            )

        self._registrations.append(_Registration(cmdline, make_result, occurrences))

    def __call__(
        self, cmdline: tuple[str, ...], **kwargs: object
    ) -> CommandResult[str] | CommandResult[bytes]:
        # Records the call once and resolves `side_effect`/`return_value`
        # in the same step — no need to call `self.mock` a second time to
        # get "the" resolved value; this *is* it.
        recorded_result = self.mock(cmdline, **kwargs)

        if self._registrations:
            result = self._resolve(cmdline)
        else:
            result = recorded_result
            # No `.register(...)` calls and nothing else configured on the
            # underlying `Mock` — `recorded_result` is just a plain,
            # auto-vivified `Mock`, not a `CommandResult`. Checking the
            # actual return type (rather than trying to detect "was
            # return_value explicitly set", which has no reliable sentinel —
            # see the `.register(...)` guard above) is what lets us raise a
            # clear, actionable error here instead of letting
            # `msgspec.structs.replace` below fail with a confusing,
            # off-topic `TypeError`.
            if not isinstance(result, CommandResult):
                msg = (
                    "RunMock: no `.register(...)` calls configured, and `run_mock.mock` has no "
                    "`side_effect`/`return_value` that returns a `CommandResult`. Either call "
                    "`run_mock.register(...)`, or set `run_mock.mock.side_effect`/"
                    "`run_mock.mock.return_value` to a `CommandResult` built with "
                    "`make_command_result(...)`."
                )
                raise TypeError(msg)

        if not kwargs.get("capture_output"):
            result = msgspec.structs.replace(result, stdout=None, stderr=None)
        return result

    def _resolve(self, cmdline: tuple[str, ...]) -> CommandResult[str] | CommandResult[bytes]:
        best: _Registration | None = None
        for reg in self._registrations:
            if cmdline[: len(reg.prefix)] == reg.prefix and (
                best is None or len(reg.prefix) > len(best.prefix)
            ):
                if reg.occurrences is not None and reg.occurrences <= 0:
                    continue
                best = reg
        if best is None:
            msg = f"RunMock: no registration matches {cmdline!r}"
            raise AssertionError(msg)
        if best.occurrences is not None:
            best.occurrences -= 1
        return best.make_result(cmdline)

    # Explicit, fixed forwarding to the underlying Mock — not a blanket
    # __getattr__, so a genuine typo raises AttributeError instead of
    # silently returning a fresh child Mock.
    def assert_called_with(self, *args: object, **kwargs: object) -> None:
        self.mock.assert_called_with(*args, **kwargs)

    def assert_any_call(self, *args: object, **kwargs: object) -> None:
        self.mock.assert_any_call(*args, **kwargs)

    def assert_called_once_with(self, *args: object, **kwargs: object) -> None:
        self.mock.assert_called_once_with(*args, **kwargs)

    @property
    def call_args(self):
        # Untyped, mirroring unittest.mock's own (untyped) property.
        return self.mock.call_args

    @property
    def call_args_list(self):
        return self.mock.call_args_list

    @property
    def call_count(self) -> int:
        return self.mock.call_count

    def reset_mock(self, *args: object, **kwargs: object) -> None:
        self.mock.reset_mock(*args, **kwargs)
        self._registrations.clear()
