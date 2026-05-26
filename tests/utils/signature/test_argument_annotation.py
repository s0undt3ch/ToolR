"""Tests for ArgumentAnnotation class and arg function."""

from __future__ import annotations

import pytest

from toolr._exc import ToolrDeprecationWarning
from toolr.utils._signature import ArgSection
from toolr.utils._signature import ArgumentAnnotation
from toolr.utils._signature import arg
from toolr.utils._signature import arg_section


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


def test_arg_section_carries_title_and_description():
    """arg_section() returns an ArgSection with title and optional description."""
    section = arg_section("Logging", description="Control verbosity.")
    assert isinstance(section, ArgSection)
    assert section.title == "Logging"
    assert section.description == "Control verbosity."


def test_arg_section_omits_description_when_not_passed():
    section = arg_section("Network")
    assert section.title == "Network"
    assert section.description is None


def test_arg_accepts_help_section_and_other_new_kwargs():
    """The new kwargs land on the resulting annotation as-is."""
    section = arg_section("Logging")
    annotation = arg(
        env="DEPLOY_TOKEN",
        hide=True,
        display_order=3,
        conflicts_with=["quiet"],
        requires=["log_file"],
        help_section=section,
        must_be_file=True,
    )
    assert annotation.env == "DEPLOY_TOKEN"
    assert annotation.hide is True
    assert annotation.display_order == 3
    assert annotation.conflicts_with == ["quiet"]
    assert annotation.requires == ["log_file"]
    assert annotation.help_section is section
    assert annotation.must_be_file is True


@pytest.mark.parametrize(
    "kwargs",
    [
        {"required": True},
        {"choices": ["a", "b"]},
        {"nargs": "*"},
        {"action": "store_true"},
        {"group": "verbosity"},
    ],
)
def test_legacy_kwargs_emit_deprecation_warning(kwargs):
    with pytest.warns(ToolrDeprecationWarning):
        arg(**kwargs)


def test_path_constraint_kwargs_land_on_annotation():
    """`must_exist` / `must_be_file` / `must_be_dir` are first-class fields."""
    annotation = arg(must_exist=True, must_be_dir=True)
    assert annotation.must_exist is True
    assert annotation.must_be_dir is True
    assert annotation.must_be_file is False


@pytest.mark.parametrize("kwarg", ["aliases", "conflicts_with", "requires"])
def test_arg_rejects_bare_string_for_collection_kwarg(kwarg):
    """Passing a bare string where a collection is expected raises a TypeError.

    Without this guard, ``arg(conflicts_with="other")`` is silently
    accepted in Python but then dropped by the AST parser, giving a
    working-looking command whose mutex never fires.
    """
    with pytest.raises(TypeError, match=rf"`{kwarg}=`.*bare `str`.*Wrap"):
        arg(**{kwarg: "value"})


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (["-n", "--name"], ["-n", "--name"]),
        (("-n", "--name"), ["-n", "--name"]),
    ],
    ids=["list", "tuple"],
)
def test_arg_aliases_accepts_list_and_tuple(value, expected):
    """``aliases`` accepts list and tuple — both preserve declaration order."""
    annotation = arg(aliases=value)
    assert annotation.aliases == expected


@pytest.mark.parametrize("value", [{"-n", "--name"}, frozenset({"-n", "--name"})])
def test_arg_aliases_rejects_unordered_collections(value):
    """Sets are rejected because alias order drives clap short-flag assignment."""
    with pytest.raises(TypeError, match=r"`aliases=` must be a list or tuple of strings"):
        arg(aliases=value)


def test_arg_aliases_rejects_non_string_elements():
    with pytest.raises(TypeError, match=r"`aliases=` element \[0\].*got int"):
        arg(aliases=[1, 2])


@pytest.mark.parametrize("kwarg", ["conflicts_with", "requires"])
@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (["a", "b"], ["a", "b"]),
        (("a", "b"), ["a", "b"]),
        (frozenset(["solo"]), ["solo"]),
    ],
    ids=["list", "tuple", "frozenset"],
)
def test_arg_setlike_kwargs_accept_list_tuple_set(kwarg, value, expected):
    """``conflicts_with`` / ``requires`` accept list, tuple, set, or frozenset."""
    annotation = arg(**{kwarg: value})
    assert getattr(annotation, kwarg) == expected


def test_arg_conflicts_with_accepts_set_literal():
    annotation = arg(conflicts_with={"solo"})
    assert annotation.conflicts_with == ["solo"]


@pytest.mark.parametrize("kwarg", ["conflicts_with", "requires"])
def test_arg_setlike_kwargs_reject_other_collections(kwarg):
    """Generators / dicts / arbitrary iterables are rejected.

    The AST parser only extracts literal ``[...]`` / ``(...)`` /
    ``{...}`` forms; accepting fancier shapes at the Python layer
    would create a silent-drop hazard between runtime and the static
    manifest.
    """
    with pytest.raises(TypeError, match=rf"`{kwarg}=`.*got dict"):
        arg(**{kwarg: {"a": 1}})


@pytest.mark.parametrize("kwarg", ["conflicts_with", "requires"])
def test_arg_setlike_kwargs_reject_non_string_elements(kwarg):
    with pytest.raises(TypeError, match=rf"`{kwarg}=` element \[0\].*got int"):
        arg(**{kwarg: [1, 2]})
