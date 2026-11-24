"""A ``ctx.run`` stand-in for pre-commit command tests that shell out."""

from __future__ import annotations

from typing import Any

from toolr.testing import make_command_result
from toolr.utils.command import CommandResult


class RecordingRun:
    """Record each command line and return canned ``(stdout, stderr, returncode)`` results in order."""

    def __init__(self, *results: tuple[str, str, int]) -> None:
        self.calls: list[tuple[str, ...]] = []
        self._results = iter(results)

    def __call__(
        self, cmdline: tuple[str, ...], **_: Any
    ) -> CommandResult[str] | CommandResult[bytes]:
        self.calls.append(cmdline)
        stdout, stderr, returncode = next(self._results)
        return make_command_result(
            args=list(cmdline), stdout=stdout, stderr=stderr, returncode=returncode
        )

    @property
    def args(self) -> list[tuple[str, ...]]:
        """Each recorded command line without its resolved executable."""
        return [cmdline[1:] for cmdline in self.calls]
