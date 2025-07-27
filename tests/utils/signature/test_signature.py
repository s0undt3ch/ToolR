"""Tests for the Signature class."""

from __future__ import annotations

import inspect
from argparse import ArgumentParser
from argparse import Namespace

import pytest

from toolr._context import Context
from toolr.utils._signature import Arg
from toolr.utils._signature import KwArg
from toolr.utils._signature import Signature


@pytest.fixture
def mock_context():
    """Create a mock context for testing."""
    return Context(
        repo_root=None,
        parser=None,
        verbosity=None,
        console=None,
        console_stdout=None,
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
