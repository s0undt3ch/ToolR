from __future__ import annotations


class ToolrError(Exception):
    """Base exception for all Toolr errors."""


class SignatureError(ToolrError):
    """Exception raised when a function signature is invalid."""
