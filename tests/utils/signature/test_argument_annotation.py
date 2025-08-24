"""Tests for ArgumentAnnotation class and arg function."""

from __future__ import annotations

from toolr.utils._signature import ArgumentAnnotation
from toolr.utils._signature import arg


def test_argument_annotation_creation():
    """Test creating an ArgumentAnnotation."""
    annotation = ArgumentAnnotation(
        aliases=["--test", "-t"],
        required=True,
        metavar="TEST",
        action="store_true",
        choices=["a", "b", "c"],
        nargs="*",
        group="test_group",
    )
    assert annotation.aliases == ["--test", "-t"]
    assert annotation.required is True
    assert annotation.metavar == "TEST"
    assert annotation.action == "store_true"
    assert annotation.choices == ["a", "b", "c"]
    assert annotation.nargs == "*"
    assert annotation.group == "test_group"


def test_argument_annotation_defaults():
    """Test ArgumentAnnotation with default values."""
    annotation = ArgumentAnnotation()
    assert annotation.aliases is None
    assert annotation.required is None
    assert annotation.metavar is None
    assert annotation.action is None
    assert annotation.choices is None
    assert annotation.nargs is None
    assert annotation.group is None


def test_arg_function():
    """Test the arg function creates correct ArgumentAnnotation."""
    annotation = arg(
        aliases=["--test", "-t"],
        required=True,
        metavar="TEST",
        action="store_true",
        choices=["a", "b", "c"],
        nargs="*",
        group="test_group",
    )
    assert isinstance(annotation, ArgumentAnnotation)
    assert annotation.aliases == ["--test", "-t"]
    assert annotation.required is True
    assert annotation.metavar == "TEST"
    assert annotation.action == "store_true"
    assert annotation.choices == ["a", "b", "c"]
    assert annotation.nargs == "*"
    assert annotation.group == "test_group"


def test_arg_function_defaults():
    """Test arg function with default values."""
    annotation = arg()
    assert isinstance(annotation, ArgumentAnnotation)
    assert annotation.aliases is None
    assert annotation.required is None
    assert annotation.metavar is None
    assert annotation.action is None
    assert annotation.choices is None
    assert annotation.nargs is None
    assert annotation.group is None


def test_argument_annotation_mutually_exclusive_group_only():
    """Test ArgumentAnnotation with only group specified."""
    annotation = ArgumentAnnotation(group="group1")
    assert annotation.group == "group1"
    assert annotation.aliases is None
    assert annotation.required is None
    assert annotation.metavar is None
    assert annotation.action is None
    assert annotation.choices is None
    assert annotation.nargs is None


def test_arg_function_mutually_exclusive_group_only():
    """Test arg function with only group specified."""
    annotation = arg(group="group1")
    assert isinstance(annotation, ArgumentAnnotation)
    assert annotation.group == "group1"
    assert annotation.aliases is None
    assert annotation.required is None
    assert annotation.metavar is None
    assert annotation.action is None
    assert annotation.choices is None
    assert annotation.nargs is None
