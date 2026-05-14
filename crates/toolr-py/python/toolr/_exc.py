from __future__ import annotations

from collections.abc import Callable


class ToolrError(Exception):
    """Base exception for all Toolr errors."""


class SignatureParameterError(ToolrError):
    """Exception raised when a function signature parameter is invalid."""

    def __init__(self, message: str) -> None:
        self.message = message
        super().__init__(message)


class SignatureError(SignatureParameterError):
    """Exception raised when a function signature is invalid."""

    def __init__(self, message: str, func: Callable | None = None) -> None:
        if func is not None:
            message = f"{func.__module__}.{func.__name__}: {message}"
        super().__init__(message)


class ToolrDeprecationWarning(DeprecationWarning):
    """Warning emitted for toolr APIs scheduled for removal in 1.0.

    Subclasses :class:`DeprecationWarning` so it's silenced by Python's
    default filter, but the toolr runner re-enables it explicitly so
    end users see the warning when they invoke a command.
    """
