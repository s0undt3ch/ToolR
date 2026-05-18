"""Unit-level coverage for ``toolr._decorators``.

The existing introspect/runner tests exercise `command_group` indirectly,
but several branches (the bare-callable form of `@command`, the dotted
name-with-conflicting-parent warning, the bound `@group.command`
decorator) never fire from them. This file pokes the helpers directly
so the coverage counter moves and so any regression in the decorator
surface (which is part of toolr's public API) is caught early.

The bound `@group.command` form is no longer deprecated — it is the
canonical single-file form. The bound *subgroup* form
(`parent.command_group("child", ...)`) is still on track for removal
in toolr 1.0 and continues to emit `ToolrDeprecationWarning`.
"""

from __future__ import annotations

import logging
import warnings
from collections.abc import Iterator

import pytest

from toolr._decorators import CommandGroup
from toolr._decorators import _get_command_group_storage
from toolr._decorators import command
from toolr._decorators import command_group
from toolr._exc import ToolrDeprecationWarning


@pytest.fixture
def clean_registry() -> Iterator[None]:
    """Snapshot + restore the process-wide command-group storage.

    Without this, registrations from one test leak into another and
    `command_group(name, ...)` returns the cached instance instead of
    walking the freshly-exercised code paths.
    """
    storage = _get_command_group_storage()
    saved = dict(storage)
    storage.clear()
    yield
    storage.clear()
    storage.update(saved)


# --------------------------------------------------------------------
# CommandGroup.full_name
# --------------------------------------------------------------------


def test_command_group_full_name_for_top_level_returns_just_name():
    g = CommandGroup(name="ci", title="CI", description="")
    assert g.full_name == "ci"


def test_command_group_full_name_for_nested_returns_dotted_path():
    g = CommandGroup(name="diff", title="", description="", parent="tools.docker")
    assert g.full_name == "tools.docker.diff"


# --------------------------------------------------------------------
# Bound `@group.command` decorator
#
# This is the canonical single-file form. The decorator was previously
# deprecated; the deprecation was rolled back so each test here asserts
# that NO `ToolrDeprecationWarning` fires. The still-deprecated
# bound-subgroup form (`parent.command_group("child", ...)`) is covered
# separately below.
# --------------------------------------------------------------------


def test_group_command_decorator_returns_callable_unchanged(clean_registry: None):
    del clean_registry
    g = command_group("legacy", "Legacy", description="Legacy group for tests")

    with warnings.catch_warnings():
        warnings.simplefilter("error", ToolrDeprecationWarning)

        @g.command
        def f(ctx) -> None: ...

    # The decorator's only contract is "return the function unchanged";
    # the static parser is what records the metadata downstream. Asserting
    # identity catches accidental wrapping.
    assert f.__name__ == "f"


def test_group_command_with_explicit_name_returns_decorator(clean_registry: None):
    del clean_registry
    g = command_group("legacy", "Legacy", description="Legacy group for tests")
    with warnings.catch_warnings():
        warnings.simplefilter("error", ToolrDeprecationWarning)
        decorator = g.command("my-cmd")
    assert callable(decorator)

    def f(ctx) -> None: ...

    assert decorator(f) is f


def test_parent_command_group_method_still_emits_deprecation(clean_registry: None):
    """The bound-subgroup form (`parent.command_group("child", ...)`) is
    still on track for removal in toolr 1.0 — guard the warning stays."""
    del clean_registry
    parent = command_group("legacy_parent", "Parent", description="Parent group")
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always", ToolrDeprecationWarning)
        parent.command_group("child", "Child", description="Child group")
    assert any(issubclass(w.category, ToolrDeprecationWarning) for w in caught), (
        "expected ToolrDeprecationWarning from parent.command_group(...)"
    )


# --------------------------------------------------------------------
# @command — bare callable form
# --------------------------------------------------------------------


def test_command_bare_form_returns_function_unchanged():
    @command
    def f(ctx) -> None: ...

    assert f.__name__ == "f"


def test_command_bare_form_rejects_kwargs_via_typeerror():
    # `@command def f(): ...` is the no-paren form; passing kwargs in
    # that shape is a usage error caught at decoration time.
    def f(ctx) -> None: ...

    with pytest.raises(TypeError, match="kwargs"):
        command(f, group="ci")


# --------------------------------------------------------------------
# @command(...) — parameterised form
# --------------------------------------------------------------------


def test_command_parameterised_form_returns_passthrough_decorator():
    decorator = command(group="ci")
    assert callable(decorator)

    def f(ctx) -> None: ...

    assert decorator(f) is f


def test_command_parameterised_form_with_string_first_arg():
    # The first positional may be a string name (not a callable); that
    # branches into the parameterised-form path.
    decorator = command("rename-me", group="ci")
    assert callable(decorator)

    def f(ctx) -> None: ...

    assert decorator(f) is f


# --------------------------------------------------------------------
# command_group — name parsing branches
# --------------------------------------------------------------------


def test_command_group_with_dotted_name_splits_into_parent_and_leaf(
    clean_registry: None,
):
    del clean_registry
    g = command_group("docker.diff", "Docker Diff", description="Docker diff group")
    assert g.name == "diff"
    assert g.parent == "tools.docker"
    assert g.full_name == "tools.docker.diff"


def test_command_group_dotted_name_overrides_explicit_parent_with_warning(
    clean_registry: None,
    caplog: pytest.LogCaptureFixture,
):
    del clean_registry
    with caplog.at_level(logging.WARNING, logger="toolr._decorators"):
        g = command_group("docker.diff", "Docker Diff", description="Docker diff group", parent="wrong")
    assert g.parent == "tools.docker"
    assert any("dotted path" in r.message for r in caplog.records)


def test_command_group_dotted_name_explicit_parent_matches_no_warning(
    clean_registry: None,
    caplog: pytest.LogCaptureFixture,
):
    del clean_registry
    with caplog.at_level(logging.WARNING, logger="toolr._decorators"):
        g = command_group("docker.diff", "Docker Diff", description="Docker diff", parent="docker")
    assert g.parent == "tools.docker"
    # Explicit parent matches the dotted prefix → no warning fires.
    assert not any("dotted path" in r.message for r in caplog.records)


def test_command_group_parent_kwarg_gets_tools_prefix(clean_registry: None):
    del clean_registry
    g = command_group("diff", description="Diff", parent="docker")
    assert g.parent == "tools.docker"


def test_command_group_parent_already_prefixed_left_alone(clean_registry: None):
    del clean_registry
    g = command_group("diff", description="Diff", parent="tools.docker")
    assert g.parent == "tools.docker"


def test_command_group_with_no_parent_defaults_to_tools(clean_registry: None):
    del clean_registry
    g = command_group("ci", description="CI")
    assert g.parent == "tools"


def test_command_group_blank_title_defaults_to_leaf_name(clean_registry: None):
    del clean_registry
    g = command_group("ci", description="CI")
    assert g.title == "ci"


def test_command_group_returns_existing_instance_on_second_call(
    clean_registry: None,
    caplog: pytest.LogCaptureFixture,
):
    del clean_registry
    first = command_group("ci", "CI", description="CI")
    with caplog.at_level(logging.DEBUG, logger="toolr._decorators"):
        second = command_group("ci", "CI", description="CI")
    assert second is first
