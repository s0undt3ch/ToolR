"""Tests for command group error cases."""

from __future__ import annotations

import pytest

from toolr import command_group


def test_command_group_without_description_or_docstring():
    """Test command_group error when neither description nor docstring provided."""
    with pytest.raises(ValueError, match="You must at least pass either the 'docstring' or 'description' argument"):
        command_group("test", "Test Group")


def test_command_group_with_both_docstring_and_description():
    """Test command_group error when both docstring and description provided."""
    with pytest.raises(ValueError, match="You can't pass both docstring and description or long_description"):
        command_group("test", "Test Group", description="Description", docstring="Docstring")


def test_command_group_with_docstring_parsing():
    """Test command_group with docstring parsing."""
    group = command_group(
        "docstring_test", "Docstring Test", docstring="Short description.\n\nLong description with more details."
    )

    assert group.description == "Short description."
    assert group.long_description == "Long description with more details."


def test_command_group_full_name_with_parent():
    """Test CommandGroup full_name property with parent."""
    group = command_group("child", "Child Group", "A child group", parent="tools.parent")

    assert group.full_name == "tools.parent.child"


def test_command_group_full_name_without_parent():
    """Test CommandGroup full_name property without parent."""
    group = command_group("test", "Test Group", "A test group")

    assert group.full_name == "tools.test"
