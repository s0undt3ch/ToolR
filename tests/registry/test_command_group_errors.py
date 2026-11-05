"""Tests for command group error cases."""

from __future__ import annotations

import importlib
import logging
import sys
import textwrap
from collections.abc import Callable
from pathlib import Path

import pytest

from toolr import command_group
from toolr._decorators import _get_command_group_storage


def test_command_group_without_description_or_docstring_creates_the_group():
    """A missing description is the manifest build's concern, not the runtime's."""
    group = command_group("test", "Test Group")

    assert group.title == "Test Group"
    assert group.description == ""
    assert group.long_description is None
    assert _get_command_group_storage()["tools.test"] is group
    assert command_group("test", "Test Group") is group


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


def test_contradictory_call_raises_even_when_the_group_exists():
    existing = command_group("dup", "T", "d")

    with pytest.raises(
        ValueError, match="You can't pass both docstring and description or long_description"
    ):
        command_group("dup", "T", description="d", docstring="x")
    with pytest.raises(
        ValueError, match="You can't pass both docstring and description or long_description"
    ):
        command_group("dup", "T", long_description="l", docstring="x")

    assert command_group("dup", "T", "d") is existing


def test_title_only_refetch_returns_the_existing_group(caplog: pytest.LogCaptureFixture):
    existing = command_group("ci", "CI", "Continuous integration.")

    with caplog.at_level(logging.DEBUG, logger="toolr._decorators"):
        again = command_group("ci", "CI")
        bare = command_group("ci")

    assert again is existing
    assert bare is existing
    assert existing.description == "Continuous integration."
    assert not [r for r in caplog.records if r.levelno >= logging.WARNING]


def test_title_only_refetch_of_a_dotted_group():
    existing = command_group("ci.helm", "T", "d")

    assert command_group("ci.helm", "T") is existing


def test_refetch_with_a_different_title_is_logged(caplog: pytest.LogCaptureFixture):
    existing = command_group("ci", "CI", "d")

    with caplog.at_level(logging.DEBUG, logger="toolr._decorators"):
        again = command_group("ci", "Other")

    assert again is existing
    assert "title" in caplog.records[-1].getMessage()


@pytest.fixture
def make_package(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> Callable[[dict[str, str]], None]:
    def factory(files: dict[str, str]) -> None:
        for rel, body in files.items():
            target = tmp_path / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(textwrap.dedent(body))
        monkeypatch.syspath_prepend(str(tmp_path))
        for name in list(sys.modules):
            if name == "pkgtools" or name.startswith("pkgtools."):
                monkeypatch.delitem(sys.modules, name)
        importlib.invalidate_caches()

    return factory


def test_package_init_declares_and_submodule_refetches_by_title(make_package):
    make_package(
        {
            "pkgtools/__init__.py": "",
            "pkgtools/ci/__init__.py": '''
                """CI helpers."""
                from toolr import command_group

                group = command_group("ci", "CI", docstring=__doc__)
            ''',
            "pkgtools/ci/lint.py": """
                from toolr import command_group

                group = command_group("ci", "CI")
            """,
        }
    )

    declared = importlib.import_module("pkgtools.ci")
    refetched = importlib.import_module("pkgtools.ci.lint")

    assert refetched.group is declared.group
    assert _get_command_group_storage()["tools.ci"] is declared.group


def test_title_only_submodule_imports_alone_when_a_sibling_creates_the_group(make_package):
    make_package(
        {
            "pkgtools/__init__.py": "",
            "pkgtools/ci/__init__.py": "",
            "pkgtools/ci/a.py": """
                from toolr import command_group

                group = command_group("ci", "CI", "Continuous integration.")
            """,
            "pkgtools/ci/lint.py": """
                from toolr import command_group

                group = command_group("ci", "CI")
            """,
        }
    )

    lint = importlib.import_module("pkgtools.ci.lint")

    assert "pkgtools.ci.a" not in sys.modules
    assert lint.group is _get_command_group_storage()["tools.ci"]
    assert lint.group.description == ""


def test_identical_redeclaration_returns_the_group_without_a_conflict_record(
    caplog: pytest.LogCaptureFixture,
):
    first = command_group("same", "T", "d", long_description="l")

    with caplog.at_level(logging.DEBUG, logger="toolr._decorators"):
        second = command_group("same", "T", "d", long_description="l")

    assert second is first
    assert not [r for r in caplog.records if "ignoring" in r.getMessage()]


def test_conflicting_redeclaration_returns_the_first_group_and_logs_at_debug(
    caplog: pytest.LogCaptureFixture,
):
    first = command_group("clash", "T", "first")

    with caplog.at_level(logging.DEBUG, logger="toolr._decorators"):
        second = command_group("clash", "T", "second")

    assert second is first
    assert second.description == "first"
    assert not [r for r in caplog.records if r.levelno >= logging.WARNING]
    conflicts = [
        r for r in caplog.records if r.levelno == logging.DEBUG and "ignoring" in r.getMessage()
    ]
    assert len(conflicts) == 1
    message = conflicts[0].getMessage()
    assert "tools.clash" in message
    assert "description" in message
    assert "title" not in message


def test_conflicting_redeclaration_names_every_differing_field(caplog: pytest.LogCaptureFixture):
    command_group("many", "T", "d", long_description="l")

    with caplog.at_level(logging.DEBUG, logger="toolr._decorators"):
        command_group("many", "U", "e", long_description="m")

    message = caplog.records[-1].getMessage()
    assert "title" in message
    assert "description" in message
    assert "long_description" in message
