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
import os
import re
import subprocess
import sys
import textwrap
import warnings
from collections.abc import Iterator

import pytest

from toolr._decorators import CommandGroup
from toolr._decorators import _get_command_group_storage
from toolr._decorators import command
from toolr._decorators import command_group
from toolr._exc import ToolrDeprecationWarning


@pytest.fixture(autouse=True)
def _clean_registry() -> Iterator[None]:
    """Snapshot + restore the process-wide command-group storage.

    Autouse: every test in this module exercises ``command_group``
    registration, and without isolation a registration from one test
    leaks into the next so ``command_group(name, ...)`` returns the
    cached instance instead of walking the freshly-exercised code
    paths. The fixture has no value to inject, so consumers don't need
    to name it as a parameter.
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


def test_group_command_decorator_returns_callable_unchanged():
    g = command_group("legacy", "Legacy", description="Legacy group for tests")

    with warnings.catch_warnings():
        warnings.simplefilter("error", ToolrDeprecationWarning)

        @g.command
        def f(ctx) -> None:
            """Do it."""

    # The decorator's only contract is "return the function unchanged";
    # the static parser is what records the metadata downstream. Asserting
    # identity catches accidental wrapping.
    assert f.__name__ == "f"


def test_group_command_with_explicit_name_returns_decorator():
    g = command_group("legacy", "Legacy", description="Legacy group for tests")
    with warnings.catch_warnings():
        warnings.simplefilter("error", ToolrDeprecationWarning)
        decorator = g.command("my-cmd")
    assert callable(decorator)

    def f(ctx) -> None:
        """Do it."""

    assert decorator(f) is f


def test_group_command_with_name_keyword_registers_under_that_name():
    g = command_group("legacy", "Legacy", description="Legacy group for tests")

    @g.command(name="collect")
    def collect_data(ctx) -> None:
        """Do it."""

    commands = g.get_commands()
    # The `name=` keyword wins over the hyphenated function name.
    assert "collect" in commands
    assert "collect-data" not in commands


def test_group_command_empty_parens_registers_under_function_name():
    g = command_group("legacy", "Legacy", description="Legacy group for tests")

    @g.command()
    def collect_data(ctx) -> None:
        """Do it."""

    assert "collect-data" in g.get_commands()


def test_group_command_duplicate_name_overrides_and_logs(caplog: pytest.LogCaptureFixture):
    g = command_group("legacy", "Legacy", description="Legacy group for tests")

    @g.command(name="dup")
    def first(ctx) -> None:
        """Do it."""

    with caplog.at_level(logging.DEBUG, logger="toolr._decorators"):

        @g.command(name="dup")
        def second(ctx) -> None:
            """Do it."""

    # The second registration overrides the first under the same name.
    assert g.get_commands()["dup"].__name__ == "second"
    assert any("already exists" in r.message for r in caplog.records)


def test_group_command_rejects_positional_and_name_keyword():
    # A single `name` parameter means Python itself rejects passing the
    # name both positionally and by keyword — no explicit guard needed.
    g = command_group("legacy", "Legacy", description="Legacy group for tests")
    with pytest.raises(TypeError, match="multiple values for argument 'name'"):
        g.command("positional", name="keyword")


def test_parent_command_group_method_still_emits_deprecation():
    """The bound-subgroup form (`parent.command_group("child", ...)`) is
    still on track for removal in toolr 1.0 — guard the warning stays."""
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
    def f(ctx) -> None:
        """Do it."""

    assert f.__name__ == "f"


def test_command_bare_form_rejects_kwargs_via_typeerror():
    # `@command def f(): ...` is the no-paren form; passing kwargs in
    # that shape is a usage error caught at decoration time.
    def f(ctx) -> None:
        """Do it."""

    with pytest.raises(TypeError, match="kwargs"):
        command(f, group="ci")


# --------------------------------------------------------------------
# @command(...) — parameterised form
# --------------------------------------------------------------------


def test_command_parameterised_form_returns_passthrough_decorator():
    decorator = command(group="ci")
    assert callable(decorator)

    def f(ctx) -> None:
        """Do it."""

    assert decorator(f) is f


def test_command_parameterised_form_with_string_first_arg():
    # The first positional may be a string name (not a callable); that
    # branches into the parameterised-form path.
    decorator = command("rename-me", group="ci")
    assert callable(decorator)

    def f(ctx) -> None:
        """Do it."""

    assert decorator(f) is f


def test_command_parameterised_form_with_name_keyword():
    # `name=` is the keyword spelling of the same first positional; it
    # mirrors the bound `@group.command(name=...)` form.
    decorator = command(name="rename-me", group="ci")
    assert callable(decorator)

    def f(ctx) -> None:
        """Do it."""

    assert decorator(f) is f


def test_command_rejects_positional_and_name_keyword():
    # Single `name` parameter → Python rejects the redundant override.
    with pytest.raises(TypeError, match="multiple values for argument 'name'"):
        command("positional", name="keyword", group="ci")


# --------------------------------------------------------------------
# command_group — name parsing branches
# --------------------------------------------------------------------


def test_command_group_with_dotted_name_splits_into_parent_and_leaf():
    g = command_group("docker.diff", "Docker Diff", description="Docker diff group")
    assert g.name == "diff"
    assert g.parent == "tools.docker"
    assert g.full_name == "tools.docker.diff"


def test_command_group_dotted_name_overrides_explicit_parent_with_warning(
    caplog: pytest.LogCaptureFixture,
):
    with caplog.at_level(logging.WARNING, logger="toolr._decorators"):
        g = command_group(
            "docker.diff", "Docker Diff", description="Docker diff group", parent="wrong"
        )
    assert g.parent == "tools.docker"
    assert any("dotted path" in r.message for r in caplog.records)


def test_command_group_dotted_name_explicit_parent_matches_no_warning(
    caplog: pytest.LogCaptureFixture,
):
    with caplog.at_level(logging.WARNING, logger="toolr._decorators"):
        g = command_group("docker.diff", "Docker Diff", description="Docker diff", parent="docker")
    assert g.parent == "tools.docker"
    # Explicit parent matches the dotted prefix → no warning fires.
    assert not any("dotted path" in r.message for r in caplog.records)


def test_command_group_parent_kwarg_gets_tools_prefix():
    g = command_group("diff", description="Diff", parent="docker")
    assert g.parent == "tools.docker"


def test_command_group_parent_already_prefixed_left_alone():
    g = command_group("diff", description="Diff", parent="tools.docker")
    assert g.parent == "tools.docker"


def test_command_group_with_no_parent_defaults_to_tools():
    g = command_group("ci", description="CI")
    assert g.parent == "tools"


def test_command_group_blank_title_stays_empty_when_no_docstring():
    # With no `docstring=` to populate it, an unset title stays empty —
    # clap renders the group name on its own in the parent listing rather
    # than duplicating the leaf name as a redundant "about" string.
    g = command_group("ci", description="CI")
    assert g.title == ""


def test_command_group_docstring_first_paragraph_becomes_title():
    # Single-paragraph docstring: title takes the short_description,
    # description renders to the same paragraph (no body / sections).
    g = command_group("ci", docstring="Short title for parent listing.")
    assert g.title == "Short title for parent listing."
    assert g.description == "Short title for parent listing."


def test_command_group_docstring_long_paragraph_becomes_description():
    # `description` is the Rust-rendered ``full_description`` — short
    # paragraph + long body, no trailing newline when no sections follow.
    # The leading short paragraph repeats the title on purpose so
    # ``--help`` re-states the blurb the parent listing already shows.
    g = command_group(
        "ci",
        docstring="Short title for parent listing.\n\nLonger prose shown by --help only.",
    )
    assert g.title == "Short title for parent listing."
    assert g.description == (
        "Short title for parent listing.\n\nLonger prose shown by --help only."
    )


def test_command_group_explicit_title_overrides_docstring_short():
    g = command_group(
        "ci",
        "Explicit Title",
        docstring="Short title from docstring.\n\nLong paragraph kept as description.",
    )
    assert g.title == "Explicit Title"
    assert g.description == "Short title from docstring.\n\nLong paragraph kept as description."


def test_command_group_docstring_with_notes_section_renders_into_description():
    # Section headers (Notes/Examples/Warnings/…) feed clap's
    # `long_about` via the same Rust-rendered ``full_description``
    # string. This is the user-visible payoff: ``toolr <group> --help``
    # now shows everything the docstring carries, not just the long
    # paragraph.
    g = command_group(
        "ci",
        docstring=(
            "Short title.\n\n"
            "Long body explaining the group.\n\n"
            "Notes:\n"
            "    Heads-up about something subtle.\n"
            "    Second note.\n"
        ),
    )
    assert g.title == "Short title."
    assert "Long body explaining the group." in g.description
    assert "## Notes" in g.description
    assert "Heads-up about something subtle." in g.description
    assert "Second note." in g.description


def test_command_group_returns_existing_instance_on_second_call(
    caplog: pytest.LogCaptureFixture,
):
    first = command_group("ci", "CI", description="CI")
    with caplog.at_level(logging.DEBUG, logger="toolr._decorators"):
        second = command_group("ci", "CI", description="CI")
    assert second is first


# --------------------------------------------------------------------
# Commands must have a docstring summary (#501)
# --------------------------------------------------------------------


def _bare(ctx) -> None:
    pass


def _empty(ctx) -> None:
    """"""


def _blank(ctx) -> None:
    """ """


def _sections_only(ctx, name: str) -> None:
    """
    Args:
        name: Who to greet.
    """


@pytest.fixture(params=[_bare, _empty, _blank, _sections_only], ids=lambda f: f.__name__)
def undocumented(request: pytest.FixtureRequest):
    """Each docstring shape that gives a command no summary line."""
    return request.param


@pytest.mark.parametrize(
    "register",
    [
        pytest.param(
            lambda f: command_group("doc", "Doc", description="Doc").command(f), id="group-bare"
        ),
        pytest.param(
            # A lambda so the group registers inside the test, where the
            # autouse fixture isolates the registry, not at collection.
            lambda f: command_group("doc", "Doc", description="Doc").command("x")(f),  # noqa: PLW0108
            id="group-named",
        ),
        pytest.param(command, id="command-bare"),
        pytest.param(command(group="doc"), id="command-grouped"),
    ],
)
def test_command_without_docstring_summary_is_rejected(register, undocumented):
    with pytest.raises(
        ValueError,
        match=re.escape(f"{undocumented.__module__}::{undocumented.__qualname__}: add a docstring"),
    ):
        register(undocumented)


def test_command_with_docstring_summary_is_accepted():
    def documented(ctx) -> None:
        """Has a summary."""

    g = command_group("doc", "Doc", description="Doc")
    assert g.command(documented) is documented
    assert command(documented) is documented


@pytest.mark.parametrize(
    ("flags", "env"),
    [
        pytest.param(["-OO"], {}, id="flag"),
        pytest.param([], {"PYTHONOPTIMIZE": "2"}, id="env"),
    ],
)
@pytest.mark.parametrize(
    "declaration",
    [
        pytest.param(
            """
            from toolr import command_group

            command_group("doc", "Doc", docstring=__doc__)
            """,
            id="group-docstring",
        ),
        pytest.param(
            """
            from toolr import command

            @command
            def documented(ctx):
                \"\"\"Has a summary.\"\"\"
            """,
            id="command",
        ),
    ],
)
def test_stripped_docstrings_are_refused(flags, env, declaration):
    # Under `-OO` every docstring is gone, so a documented command looks
    # undocumented; refuse outright rather than skip the check.
    script = '"""Module docs."""\n' + textwrap.dedent(declaration)
    result = subprocess.run(  # noqa: S603  # the interpreter and script are test-controlled
        [sys.executable, *flags, "-c", script],
        capture_output=True,
        text=True,
        check=False,
        env={**os.environ, **env},
    )
    assert result.returncode != 0
    assert "toolr can't run under `python -OO` or `PYTHONOPTIMIZE=2`" in result.stderr
