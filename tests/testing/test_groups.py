"""`CommandsTester`'s port of the manifest build's command-group checks."""

from __future__ import annotations

import sys
import textwrap
from collections.abc import Callable
from pathlib import Path
from types import ModuleType

import pytest

import toolr
from toolr.testing._groups import GroupDeclaration
from toolr.testing._groups import _format_contradictory_groups
from toolr.testing._groups import _format_undescribed_groups
from toolr.testing._groups import _group_declarations
from toolr.testing._groups import _undescribed_groups


@pytest.fixture
def make_tools_dir(tmp_path: Path) -> Callable[..., Path]:
    """Factory: write `{relative path: source}` under a fresh `tools/`."""

    def _make(files: dict[str, str]) -> Path:
        tools = tmp_path / "tools"
        for name, source in files.items():
            path = tools / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(textwrap.dedent(source))
        return tools

    return _make


def test_paths_follow_the_parsers_resolution(make_tools_dir: Callable[..., Path]) -> None:
    tools = make_tools_dir(
        {
            "a.py": """
                ci = command_group("ci", "CI", "CI.")
                command_group("bare", "Bare", "Bare.")
                x, y = command_group("skipped", "S", "S.")
                z = other("ignored")
                z = 1
                pass
            """,
            "b.py": """
                img = ci.command_group("image", "Image", "Image.")
                kw = command_group("kw", "Kw", "Kw.", parent=img)
                lit = command_group("lit", "Lit", "Lit.", parent="ci")
                dotted = command_group("ci.dot", "Dot", "Dot.", parent="ignored")
                nested = bare.command_group("kid", "Kid", "Kid.")
                unknown = nope.command_group("orphan", "Orphan", "Orphan.")
                call = make().command_group("deep", "Deep", "Deep.", parent="ci")
                command_group(NAME, "Name", "Name.")
            """,
            "pkg/__init__.py": """
                command_group("pkg", "Pkg", "Pkg.")
            """,
            ".hidden/m.py": """
                command_group("hidden", "Hidden", "Hidden.")
            """,
            "pkg/data.txt": "command_group('text', 'Text', 'Text.')",
        }
    )
    assert [(d.path, d.module) for d in _group_declarations(tools)] == [
        ("ci", "tools.a"),
        ("bare", "tools.a"),
        ("ci.image", "tools.b"),
        ("ci.image.kw", "tools.b"),
        ("ci.lit", "tools.b"),
        ("ci.dot", "tools.b"),
        ("bare.kid", "tools.b"),
        ("orphan", "tools.b"),
        ("ci.deep", "tools.b"),
        ("pkg", "tools.pkg"),
    ]


def test_aliases_count_once_their_module_is_imported(
    make_tools_dir: Callable[..., Path], monkeypatch: pytest.MonkeyPatch
) -> None:
    tools = make_tools_dir(
        {
            "m.py": """
                from toolr import command_group as cg
                a = cg("a", "A", "A.")
                b: CommandGroup = command_group("b", "B", "B.")
                c: CommandGroup
                d = local("d", "D", "D.")
                obj.attr: CommandGroup = cg("e", "E", "E.")
            """,
        }
    )
    assert [d.path for d in _group_declarations(tools)] == ["b"]

    def module(name: str, file: Path | None, **globals_: object) -> None:
        mod = ModuleType(name)
        if file is not None:
            mod.__file__ = str(file)
        vars(mod).update(globals_)
        monkeypatch.setitem(sys.modules, name, mod)

    module("tools.m", tools / "m.py", cg=toolr.command_group, local=print)
    # Neither is a module under `tools`, so neither's globals are read.
    module("tools.elsewhere", tools.parent / "m.py", local=toolr.command_group)
    module("tools.builtin", None)
    assert [d.path for d in _group_declarations(tools)] == ["a", "b"]


def test_description_and_contradiction_flags(make_tools_dir: Callable[..., Path]) -> None:
    tools = make_tools_dir(
        {
            "documented.py": '''
                """Doc."""
                a = command_group("a", "A", description="A.", docstring=__doc__)
                b = command_group("b", "B", "B.", docstring="B.")
                c = command_group("c", long_description="C.", docstring=__doc__)
                d = command_group("d", "D", None, None, "D.")
                e = command_group("e", "E", description=None, docstring=__doc__)
                f = command_group("f", "F")
            ''',
            "undocumented.py": """
                g = command_group("g", "G", description="G.", docstring=__doc__)
                h = command_group("h", "H", docstring=__doc__)
            """,
        }
    )
    assert [(d.path, d.describes, d.contradictory) for d in _group_declarations(tools)] == [
        ("a", True, True),
        ("b", True, True),
        ("c", True, True),
        ("d", True, False),
        ("e", True, False),
        ("f", False, False),
        ("g", True, False),
        ("h", False, False),
    ]


def test_contradictory_groups_message() -> None:
    declarations = [
        GroupDeclaration("ci", "tools.ci", describes=True, contradictory=True),
        GroupDeclaration("ok", "tools.ok", describes=True, contradictory=False),
        GroupDeclaration("c\x1b", "tools.m", describes=True, contradictory=True),
    ]
    assert _format_contradictory_groups(declarations) == (
        "command groups with contradictory arguments (2):\n"
        "  - `ci` in tools.ci: pass `docstring=` or `description=`/`long_description=`, "
        "not both.\n"
        "  - `c\\u{1b}` in tools.m: pass `docstring=` or `description=`/`long_description=`, "
        "not both."
    )
    assert _format_contradictory_groups(declarations[1:2]) is None


def test_undescribed_groups_message() -> None:
    declarations = [
        GroupDeclaration("ci", "tools.ci.build", describes=False, contradictory=False),
        GroupDeclaration("ci", "tools.ci.lint", describes=False, contradictory=False),
        GroupDeclaration("ok", "tools.ok", describes=False, contradictory=False),
        GroupDeclaration("ci", "tools.ci.lint", describes=False, contradictory=False),
        GroupDeclaration("ok", "tools.ok2", describes=True, contradictory=False),
    ]
    assert _format_undescribed_groups(_undescribed_groups(declarations)) == (
        "command groups with no description (1):\n"
        "  - `ci` (declared in tools.ci.build, tools.ci.lint): give one declaration a "
        "description or docstring."
    )
    assert _format_undescribed_groups(_undescribed_groups(declarations[4:])) is None


def test_parent_by_position(make_tools_dir: Callable[..., Path]) -> None:
    tools = make_tools_dir(
        {
            "m.py": """
                p = command_group("p", "P", "P.")
                command_group("lit", "Lit", "Lit.", None, None, "p")
                command_group("var", "Var", "Var.", None, None, p)
            """,
        }
    )
    assert [d.path for d in _group_declarations(tools)] == ["p", "p.lit", "p.var"]


def test_unreadable_values(make_tools_dir: Callable[..., Path]) -> None:
    tools = make_tools_dir(
        {
            "m.py": """
                from elsewhere import IMPORTED
                DESC = ""
                DESC += "Desc."
                OPTS = {"description": "Opts."}
                a = command_group("a", "A", DESC)
                b = command_group("b", "B", description=IMPORTED)
                c = command_group("c", "C", docstring=make())
                d = command_group("d", "D", docstring=opts.doc)
                e = command_group("e", "E", description="")
                g = command_group("g", "G", **OPTS)
                h = command_group("h", *OPTS)
                i = command_group("i", "I", description=__doc__)
            """,
        }
    )
    assert [(d.path, d.describes, d.unreadable) for d in _group_declarations(tools)] == [
        ("a", True, False),
        ("b", True, False),
        ("c", True, False),
        ("d", True, False),
        ("e", False, False),
        ("g", False, True),
        ("h", False, True),
        ("i", False, True),
    ]


def test_unreadable_arguments_are_no_contradiction(make_tools_dir: Callable[..., Path]) -> None:
    tools = make_tools_dir(
        {
            "m.py": '''
                """Doc."""
                LONG = None
                a = command_group("a", "A", long_description=LONG, docstring="A.")
                b = command_group("b", "B", description=make(), docstring=__doc__)
                c = command_group("c", "C", "C.", docstring=DOC)
                d = command_group("d", "D", 1, docstring="D.")
            ''',
        }
    )
    assert [(d.path, d.contradictory) for d in _group_declarations(tools)] == [
        ("a", False),
        ("b", False),
        ("c", False),
        ("d", True),
    ]


def test_empty_module_docstring_is_passed_but_does_not_describe(
    make_tools_dir: Callable[..., Path],
) -> None:
    tools = make_tools_dir(
        {
            "m.py": '''
                """"""
                a = command_group("a", "A", description="X", docstring=__doc__)
                b = command_group("b", "B", docstring=__doc__)
            ''',
        }
    )
    assert [
        (d.path, d.describes, d.contradictory, d.unreadable) for d in _group_declarations(tools)
    ] == [
        ("a", True, True, True),
        ("b", False, False, True),
    ]


def test_rebinding_doc_makes_it_unreadable(make_tools_dir: Callable[..., Path]) -> None:
    tools = make_tools_dir(
        {
            "a.py": """
                __doc__ = "A."
                g = command_group("a", "A", docstring=__doc__)
            """,
            "b.py": '''
                """B doc."""
                from x import y as __doc__
                g = command_group("b", "B", description="B.", docstring=__doc__)
            ''',
            "c.py": """
                try:
                    pass
                except E as __doc__:
                    pass
                g = command_group("c", "C", docstring=__doc__)
            """,
            "d.py": '''
                """D doc."""
                import os
                class K:
                    __doc__ = "k"
                def f():
                    __doc__ = "f"
                async def h():
                    __doc__ = "h"
                try:
                    pass
                except E:
                    pass
                g = command_group("d", docstring=__doc__)
                h = command_group("h", "H", description="H.", docstring=__doc__)
            ''',
            "e.py": """
                g = command_group("e", "E", docstring=__doc__)
                if True:
                    for __doc__ in []:
                        pass
            """,
        }
    )
    assert [
        (d.path, d.describes, d.contradictory, d.unreadable) for d in _group_declarations(tools)
    ] == [
        ("a", True, False, False),
        ("b", True, False, False),
        ("c", True, False, False),
        ("d", True, False, False),
        ("h", True, True, False),
        ("e", True, False, False),
    ]


def test_undescribed_groups_message_asks_for_a_readable_value() -> None:
    declarations = [
        GroupDeclaration("ci", "tools.a", describes=False, contradictory=False),
        GroupDeclaration("ci", "tools.b", describes=False, contradictory=False, unreadable=True),
    ]
    assert _format_undescribed_groups(_undescribed_groups(declarations)) == (
        "command groups with no description (1):\n"
        "  - `ci` (declared in tools.a, tools.b): give one declaration a description or "
        "docstring the static parser can read: a string literal, or the `__doc__` of a "
        "module with a docstring."
    )
