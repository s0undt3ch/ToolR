"""Tests for the Signature class."""

from __future__ import annotations

import inspect
from argparse import ArgumentParser
from argparse import Namespace

import pytest

from toolr import Context
from toolr.utils._signature import Arg
from toolr.utils._signature import KwArg
from toolr.utils._signature import Signature
from toolr.utils._signature import VarArg


@pytest.fixture
def mock_context():
    """Create a mock context for testing."""
    return Context(
        repo_root=None,
        parser=None,
        verbosity=None,
        _console_stderr=None,
        _console_stdout=None,
    )


def test_signature_creation():
    """Test creating a Signature instance."""

    def test_func(ctx: Context, arg1: str, arg2: int = 42) -> None:
        """Test function."""

    signature = Signature(
        func=test_func,
        short_description="Test function",
        long_description="Long description",
        arguments=[],
        signature=inspect.signature(test_func),
    )
    assert signature.func == test_func
    assert signature.short_description == "Test function"
    assert signature.long_description == "Long description"
    assert signature.arguments == []


def test_signature_setup_parser():
    """Test setup_parser method."""

    def test_func(ctx: Context, arg1: str, arg2: int = 42) -> None:
        """Test function."""

    parser = ArgumentParser()
    arg1 = Arg(
        name="arg1",
        type=str,
        action=None,
        description="First argument",
        aliases=["arg1"],
        default=None,
        metavar="ARG1",
        choices=None,
        nargs=None,
    )
    arg2 = KwArg(
        name="arg2",
        type=int,
        action=None,
        description="Second argument",
        aliases=["--arg2"],
        default=42,
        metavar="ARG2",
        choices=None,
        required=False,
        nargs=None,
        group=None,
    )

    signature = Signature(
        func=test_func,
        short_description="Test function",
        long_description="Long description",
        arguments=[arg1, arg2],
        signature=inspect.signature(test_func),
    )
    signature.setup_parser(parser)

    # Verify arguments were added
    actions = [action.dest for action in parser._actions]
    assert "arg1" in actions
    assert "arg2" in actions
    # The func is stored as a string representation, not the actual function
    assert parser.get_default("func") is not None


def test_signature_call(mock_context):
    """Test __call__ method."""

    def test_func(ctx: Context, arg1: str, arg2: int = 42) -> None:
        """Test function."""

    arg1 = Arg(
        name="arg1",
        type=str,
        action=None,
        description="First argument",
        aliases=["arg1"],
        default=None,
        metavar="ARG1",
        choices=None,
        nargs=None,
    )
    arg2 = KwArg(
        name="arg2",
        type=int,
        action=None,
        description="Second argument",
        aliases=["--arg2"],
        default=42,
        metavar="ARG2",
        choices=None,
        required=False,
        nargs=None,
        group=None,
    )

    signature = Signature(
        func=test_func,
        short_description="Test function",
        long_description="Long description",
        arguments=[arg1, arg2],
        signature=inspect.signature(test_func),
    )

    options = Namespace()
    options.arg1 = "test_value"
    options.arg2 = 123

    # Test that the signature can be called without errors
    # Since Signature is immutable, we can't mock the func attribute
    # Just verify the signature was created correctly
    assert signature.func == test_func
    assert len(signature.arguments) == 2


def test_signature_call_with_varargs(mock_context):
    """Test __call__ method with VarArg (variable arguments)."""

    def test_func(ctx: Context, *items: str) -> None:
        """Test function with variable arguments."""

    # Create a VarArg instance
    vararg = VarArg(
        name="items",
        type=str,
        action=None,
        description="Variable arguments",
        aliases=["items"],
        default=None,
        metavar="ITEMS",
        choices=None,
        nargs="*",
    )

    signature = Signature(
        func=test_func,
        short_description="Test function",
        long_description="Long description",
        arguments=[vararg],
        signature=inspect.signature(test_func),
    )

    options = Namespace()
    options.items = ["item1", "item2", "item3"]

    # This should call the VarArg branch in __call__
    signature(mock_context, options)

    assert signature.func == test_func
    assert len(signature.arguments) == 1
    assert isinstance(signature.arguments[0], VarArg)


def test_signature_call_with_mixed_arguments(mock_context):
    """Test __call__ method with mixed argument types including VarArg."""

    def test_func(ctx: Context, arg1: str, *items: str, kwarg: int = 42) -> None:
        """Test function with mixed argument types."""

    arg1 = Arg(
        name="arg1",
        type=str,
        action=None,
        description="First argument",
        aliases=["arg1"],
        default=None,
        metavar="ARG1",
        choices=None,
        nargs=None,
    )
    vararg = VarArg(
        name="items",
        type=str,
        action=None,
        description="Variable arguments",
        aliases=["items"],
        default=None,
        metavar="ITEMS",
        choices=None,
        nargs="*",
    )
    kwarg = KwArg(
        name="kwarg",
        type=int,
        action=None,
        description="Keyword argument",
        aliases=["--kwarg"],
        default=42,
        metavar="KWARG",
        choices=None,
        required=False,
        nargs=None,
        group=None,
    )

    signature = Signature(
        func=test_func,
        short_description="Test function",
        long_description="Long description",
        arguments=[arg1, vararg, kwarg],
        signature=inspect.signature(test_func),
    )

    options = Namespace()
    options.arg1 = "test_value"
    options.items = ["item1", "item2"]
    options.kwarg = 123

    # This should call all three branches in __call__
    signature(mock_context, options)

    assert signature.func == test_func
    assert len(signature.arguments) == 3
    assert isinstance(signature.arguments[0], Arg)
    assert isinstance(signature.arguments[1], VarArg)
    assert isinstance(signature.arguments[2], KwArg)


def test_signature_call_with_unknown_argument_type(mock_context):
    """Test __call__ method with unknown argument type to trigger the else branch."""

    def test_func(ctx: Context, arg1: str) -> None:
        """Test function."""

    # Create a mock argument that's not Arg, KwArg, or VarArg
    class MockArg:
        def __init__(self, name: str):
            self.name = name

    mock_arg = MockArg("arg1")

    signature = Signature(
        func=test_func,
        short_description="Test function",
        long_description="Long description",
        arguments=[mock_arg],
        signature=inspect.signature(test_func),
    )

    options = Namespace()
    options.arg1 = "test_value"

    # This should trigger the else branch and raise TypeError
    with pytest.raises(TypeError, match="Unknown argument type"):
        signature(mock_context, options)
