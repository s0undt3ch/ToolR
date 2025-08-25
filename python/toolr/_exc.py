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
