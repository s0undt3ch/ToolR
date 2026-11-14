from __future__ import annotations


class ToolrError(Exception):
    """Base exception for all Toolr errors."""


class SpecError(Exception):
    """Raised when the runner spec, or the command it names, can't be used."""


class NoCurrentContextError(ToolrError):
    """Raised by `current_context()` when no `Context` is set for the running task/thread."""


# Kept with no current emitter, for the next deprecation.
class ToolrDeprecationWarning(DeprecationWarning):
    """Warning emitted for toolr APIs scheduled for removal.

    Subclasses :class:`DeprecationWarning` so it's silenced by Python's
    default filter, but the toolr runner re-enables it explicitly so
    end users see the warning when they invoke a command.
    """
