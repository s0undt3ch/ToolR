"""Tests for command group error cases."""

from __future__ import annotations

import logging

import pytest

from toolr import command_group


def test_command_group_without_description_or_docstring():
    """Test command_group error when neither description nor docstring provided."""
    with pytest.raises(
        ValueError, match="You must at least pass either the 'docstring' or 'description' argument"
    ):
        command_group("test", "Test Group")


def test_command_group_with_both_docstring_and_description():
    """Test command_group error when both docstring and description provided."""
    with pytest.raises(
        ValueError, match="You can't pass both docstring and description or long_description"
    ):
        command_group("test", "Test Group", description="Description", docstring="Docstring")


def test_command_group_with_docstring_parsing():
    """Test command_group with docstring parsing.

    The docstring's first paragraph populates ``title`` (clap's ``about``,
    shown in the parent's command listing and on ``-h``). The Rust-rendered
    multi-section ``full_description`` (short + long + Examples/Notes/…)
    populates ``description`` (clap's ``long_about``, shown on ``--help``).
    The leading short paragraph in the long form repeats the title on
    purpose so ``--help`` re-states the blurb readers also see in the
    parent listing. An explicit positional title still overrides the short
    slot.
    """
    group = command_group(
        "docstring_test",
        "Docstring Test",
        docstring="Short description.\n\nLong description with more details.",
    )

    assert group.title == "Docstring Test"
    assert group.description == ("Short description.\n\nLong description with more details.")
    assert group.long_description is None


def test_command_group_full_name_with_parent():
    """Test CommandGroup full_name property with parent."""
    group = command_group("child", "Child Group", "A child group", parent="tools.parent")

    assert group.full_name == "tools.parent.child"


def test_command_group_full_name_without_parent():
    """Test CommandGroup full_name property without parent."""
    group = command_group("test", "Test Group", "A test group")

    assert group.full_name == "tools.test"


def test_invalid_call_raises_even_when_the_group_exists():
    existing = command_group("dup", "T", "d")

    with pytest.raises(
        ValueError, match="You must at least pass either the 'docstring' or 'description' argument"
    ):
        command_group("dup", "T")
    with pytest.raises(
        ValueError, match="You can't pass both docstring and description or long_description"
    ):
        command_group("dup", "T", description="d", docstring="x")

    assert command_group("dup", "T", "d") is existing


def test_invalid_call_raises_for_an_existing_dotted_group():
    command_group("ci.helm", "T", "d")

    with pytest.raises(ValueError, match="You must at least pass"):
        command_group("ci.helm", "T")


def test_identical_redeclaration_returns_the_group_without_a_warning(
    caplog: pytest.LogCaptureFixture,
):
    first = command_group("same", "T", "d", long_description="l")

    with caplog.at_level(logging.WARNING):
        second = command_group("same", "T", "d", long_description="l")

    assert second is first
    assert caplog.records == []


def test_conflicting_redeclaration_returns_the_first_group_and_warns(
    caplog: pytest.LogCaptureFixture,
):
    first = command_group("clash", "T", "first")

    with caplog.at_level(logging.WARNING):
        second = command_group("clash", "T", "second")

    assert second is first
    assert second.description == "first"
    warnings = [r for r in caplog.records if r.levelno == logging.WARNING]
    assert len(warnings) == 1
    message = warnings[0].getMessage()
    assert "tools.clash" in message
    assert "description" in message
    assert "title" not in message


def test_conflicting_redeclaration_names_every_differing_field(caplog: pytest.LogCaptureFixture):
    command_group("many", "T", "d", long_description="l")

    with caplog.at_level(logging.WARNING):
        command_group("many", "U", "e", long_description="m")

    message = caplog.records[-1].getMessage()
    assert "title" in message
    assert "description" in message
    assert "long_description" in message
