"""Tests for the Arg class."""

from __future__ import annotations

from argparse import ArgumentParser

from toolr.utils._signature import Arg


def test_arg_creation():
    """Test creating an Arg instance."""
    arg_instance = Arg(
        name="test",
        type=str,
        action=None,
        description="Test argument",
        aliases=["test"],
        default=None,
        metavar="TEST",
        choices=None,
        nargs=None,
    )
    assert arg_instance.name == "test"
    assert arg_instance.type is str
    assert arg_instance.description == "Test argument"
    assert arg_instance.aliases == ["test"]


def test_arg_build_parser_kwargs():
    """Test _build_parser_kwargs method."""
    arg_instance = Arg(
        name="test",
        type=str,
        action=None,
        description="Test argument",
        aliases=["test"],
        default="default_value",
        metavar="TEST",
        choices=["a", "b", "c"],
        nargs=None,
    )
    kwargs = arg_instance._build_parser_kwargs()
    assert kwargs["help"] == "Test argument"
    assert kwargs["action"] is None
    assert kwargs["type"] is str
    assert kwargs["metavar"] == "TEST"
    assert kwargs["default"] == "default_value"
    assert kwargs["choices"] == ["a", "b", "c"]


def test_arg_build_parser_kwargs_store_true():
    """Test _build_parser_kwargs with store_true action."""
    arg_instance = Arg(
        name="test",
        type=str,
        action="store_true",
        description="Test argument",
        aliases=["test"],
        default=None,
        metavar="TEST",
        choices=None,
        nargs=None,
    )
    kwargs = arg_instance._build_parser_kwargs()
    assert kwargs["help"] == "Test argument"
    assert kwargs["action"] == "store_true"
    assert "type" not in kwargs
    assert "metavar" not in kwargs


def test_arg_setup_parser():
    """Test setup_parser method."""
    parser = ArgumentParser()
    arg_instance = Arg(
        name="test",
        type=str,
        action=None,
        description="Test argument",
        aliases=["--test", "-t"],
        default="default_value",
        metavar="TEST",
        choices=["a", "b", "c"],
        nargs=None,
    )
    arg_instance.setup_parser(parser)
    # Verify the argument was added by checking parser actions
    actions = [action.dest for action in parser._actions]
    assert "test" in actions
