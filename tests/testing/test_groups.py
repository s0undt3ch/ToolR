"""`CommandsTester`'s port of the manifest build's command-group checks."""

from __future__ import annotations

import textwrap
from collections.abc import Callable
from pathlib import Path

import pytest

from toolr.testing._groups import GroupDeclaration
from toolr.testing._groups import _format_contradictory_groups
from toolr.testing._groups import _format_undescribed_groups
from toolr.testing._groups import _group_declarations


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
    assert _format_undescribed_groups(declarations) == (
        "command groups with no description (1):\n"
        "  - `ci` (declared in tools.ci.build, tools.ci.lint): give one declaration a "
        "description or docstring."
    )
    assert _format_undescribed_groups(declarations[4:]) is None
