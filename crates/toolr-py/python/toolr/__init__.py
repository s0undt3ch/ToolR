"""ToolR Python package.

Importable surface: :class:`toolr.Context`, :func:`toolr.command_group`,
:func:`toolr.command`, :func:`toolr.arg`, :func:`toolr.arg_section`,
:func:`toolr.current_context`. ``DispatchCommand`` lives in :mod:`toolr.sources`.

Implementation modules (not part of the user-facing API):

- ``toolr._runner``: invoked by the toolr binary via
  ``python -m toolr._runner``; reads ``$TOOLR_SPEC_FILE`` and dispatches
  into user code.
"""

from __future__ import annotations

from toolr._context import Context
from toolr._context import current_context
from toolr._decorators import command
from toolr._decorators import command_group
from toolr.utils._signature import ArgSection
from toolr.utils._signature import arg
from toolr.utils._signature import arg_section

# Resolved lazily by `__getattr__` below.
__version__: str

__all__ = [
    "ArgSection",
    "Context",
    "__version__",
    "arg",
    "arg_section",
    "command",
    "command_group",
    "current_context",
]


def __getattr__(name: str) -> str:
    # Deferred: `importlib.metadata` is a large share of `import toolr`; few callers read the version.
    if name == "__version__":
        import importlib.metadata  # noqa: PLC0415

        try:
            version = importlib.metadata.version("toolr-py")
        except importlib.metadata.PackageNotFoundError:
            version = "0.0.0.not-installed"
        globals()["__version__"] = version
        return version
    msg = f"module {__name__!r} has no attribute {name!r}"
    raise AttributeError(msg)
