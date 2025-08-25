"""Tests for the KwArg class."""

from __future__ import annotations

from toolr.utils._signature import KwArg


def test_kwarg_creation():
    """Test creating a KwArg instance."""
    kwarg_instance = KwArg(
        name="test",
        type=str,
        action=None,
        description="Test argument",
        aliases=["--test"],
        default="default_value",
        metavar="TEST",
        choices=["a", "b", "c"],
        required=True,
        nargs=None,
        group=None,
    )
    assert kwarg_instance.name == "test"
    assert kwarg_instance.type is str
    assert kwarg_instance.description == "Test argument"
    assert kwarg_instance.required is True


def test_kwarg_build_parser_kwargs():
    """Test _build_parser_kwargs method."""
    kwarg_instance = KwArg(
        name="test",
        type=str,
        action=None,
        description="Test argument",
        aliases=["--test"],
        default="default_value",
        metavar="TEST",
        choices=["a", "b", "c"],
        required=True,
        nargs=None,
        group=None,
    )
    kwargs = kwarg_instance._build_parser_kwargs()
    assert kwargs["help"] == "Test argument"
    assert kwargs["action"] is None
    assert kwargs["type"] is str
    assert kwargs["metavar"] == "TEST"
    assert kwargs["default"] == "default_value"
    assert kwargs["choices"] == ["a", "b", "c"]
    assert kwargs["required"] is True
