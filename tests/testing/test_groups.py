"""`CommandsTester`'s port of the manifest build's command-group checks."""

from __future__ import annotations

import ast
import textwrap
from collections.abc import Callable
from pathlib import Path

import pytest

from toolr.testing._groups import _MAX_REEXPORT_DEPTH
from toolr.testing._groups import GroupDeclaration
from toolr.testing._groups import _Binding
from toolr.testing._groups import _binding
from toolr.testing._groups import _format_contradictory_groups
from toolr.testing._groups import _format_undescribed_groups
from toolr.testing._groups import _group_declarations
from toolr.testing._groups import _ImportTable
from toolr.testing._groups import _Memo
from toolr.testing._groups import _unbound_group_calls
from toolr.testing._groups import _undescribed_groups
from toolr.testing._shapes import GROUP_BINDING
from toolr.testing._shapes import METHOD_GROUP
from toolr.testing._shapes import _unsupported_group_bindings


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
                z = other("ignored")
                z = 1
                pass
            """,
            "b.py": """
                img = command_group("ci.image", "Image", "Image.")
                kw = command_group("kw", "Kw", "Kw.", parent=img)
                lit = command_group("lit", "Lit", "Lit.", parent="ci")
                dotted = command_group("ci.dot", "Dot", "Dot.", parent="ignored")
                dotted2 = command_group("bare.kid", "Kid", "Kid.")
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
        ("pkg", "tools.pkg"),
    ]


GROUP = 'g = cg("{0}", "{0}", "Described.")\n'

# Hand-written table, not run against the Rust binary: the parity corpus
# (tests/e2e/test_parser_tester_parity.py) is what proves the two resolvers agree.
ALIAS_CASES = {
    "import as": (
        {"m.py": "from toolr import command_group as cg\n" + GROUP.format("a")},
        ["a"],
    ),
    "submodule": (
        {"m.py": "from toolr._decorators import command_group as cg\n" + GROUP.format("a")},
        ["a"],
    ),
    "other member": (
        {"m.py": "from toolr import command as cg\n" + GROUP.format("a")},
        [],
    ),
    "module attribute": (
        {"m.py": "import toolr as t\ncg = t.command_group\n" + GROUP.format("a")},
        ["a"],
    ),
    "dotted import binds the top-level name": (
        {"m.py": "import toolr.types\ncg = toolr.command_group\n" + GROUP.format("a")},
        ["a"],
    ),
    "attribute of a tools module": (
        {
            "m.py": "import tools.lib as lib\ncg = lib.cg\n" + GROUP.format("a"),
            "lib.py": "from toolr import command_group as cg\n",
        },
        ["a"],
    ),
    "attribute of an unbound name": (
        {"m.py": "cg = nothing.command_group\n" + GROUP.format("a")},
        [],
    ),
    "attribute of a call": (
        {"m.py": "cg = make().command_group\n" + GROUP.format("a")},
        [],
    ),
    "name assignment chain": (
        {
            "m.py": (
                "from toolr import command_group\nfirst = second = command_group\n"
                "cg = first\n" + GROUP.format("a")
            )
        },
        ["a"],
    ),
    "annotated alias": (
        {
            "m.py": "from toolr import command_group\ncg: object = command_group\n"
            + GROUP.format("a")
        },
        ["a"],
    ),
    "annotated alias of an attribute": (
        {"m.py": "import toolr\ncg: object = toolr.command_group\n" + GROUP.format("a")},
        ["a"],
    ),
    "annotation without a value binds nothing": (
        {"m.py": "from toolr import command_group as cg\ncg: object\n" + GROUP.format("a")},
        ["a"],
    ),
    "annotated attribute target": (
        {
            "m.py": "from toolr import command_group as cg\nobj.cg: object = cg\n"
            + GROUP.format("a")
        },
        ["a"],
    ),
    "tuple target": (
        {"m.py": "from toolr import command_group as cg\ncg, other = 1, 2\n" + GROUP.format("a")},
        ["a"],
    ),
    "assigned something else": (
        {"m.py": "from toolr import command_group as cg\ncg = cg()\n" + GROUP.format("a")},
        [],
    ),
    "local def": (
        {"m.py": "def cg(*args):\n    pass\n" + GROUP.format("a")},
        [],
    ),
    "local async def": (
        {"m.py": "async def cg(*args):\n    pass\n" + GROUP.format("a")},
        [],
    ),
    "local class": (
        {"m.py": "class cg:\n    pass\n" + GROUP.format("a")},
        [],
    ),
    "local binding after the import wins": (
        {"m.py": "from toolr import command_group as cg\ncg = None\n" + GROUP.format("a")},
        [],
    ),
    "import after the local binding wins": (
        {"m.py": "cg = None\nfrom toolr import command_group as cg\n" + GROUP.format("a")},
        ["a"],
    ),
    "literal spelling counts even when local": (
        {"m.py": 'def command_group(*args):\n    pass\ncommand_group("a", "A", "Described.")\n'},
        ["a"],
    ),
    "try body": (
        {
            "m.py": "try:\n    from toolr import command_group as cg\nexcept ImportError:\n    pass\n"
            + GROUP.format("a")
        },
        ["a"],
    ),
    "try handler binding wins": (
        {
            "m.py": (
                "try:\n    from toolr import command_group as cg\nexcept ImportError:\n    cg = None\n"
                + GROUP.format("a")
            )
        },
        [],
    ),
    "try star handler": (
        {
            "m.py": (
                "try:\n    pass\nexcept* ImportError:\n    from toolr import command_group as cg\n"
                + GROUP.format("a")
            )
        },
        ["a"],
    ),
    "try else is not read": (
        {
            "m.py": "try:\n    pass\nexcept ImportError:\n    pass\nelse:\n    from toolr import command_group as cg\n"
            + GROUP.format("a")
        },
        [],
    ),
    "type checking block": (
        {
            "m.py": "if TYPE_CHECKING:\n    from toolr import command_group as cg\n"
            + GROUP.format("a")
        },
        ["a"],
    ),
    "typing.TYPE_CHECKING block": (
        {
            "m.py": "import typing\nif typing.TYPE_CHECKING:\n    from toolr import command_group as cg\n"
            + GROUP.format("a")
        },
        ["a"],
    ),
    "type checking else is not read": (
        {
            "m.py": "if TYPE_CHECKING:\n    pass\nelse:\n    from toolr import command_group as cg\n"
            + GROUP.format("a")
        },
        [],
    ),
    "other attribute of typing": (
        {
            "m.py": "if typing.other:\n    from toolr import command_group as cg\n"
            + GROUP.format("a")
        },
        [],
    ),
    "other if is not read": (
        {"m.py": "if True:\n    from toolr import command_group as cg\n" + GROUP.format("a")},
        [],
    ),
    "del does not unbind": (
        {"m.py": "from toolr import command_group as cg\n" + GROUP.format("a") + "del cg\n"},
        ["a"],
    ),
    "from tools module": (
        {
            "m.py": "from tools.lib import cg\n" + GROUP.format("a"),
            "lib.py": "from toolr import command_group as cg\n",
        },
        ["a"],
    ),
    "relative import": (
        {
            "m.py": "from .lib import cg\n" + GROUP.format("a"),
            "lib.py": "from toolr import command_group as cg\n",
        },
        ["a"],
    ),
    "relative import of a package": (
        {
            "pkg/m.py": "from . import cg\n" + GROUP.format("a"),
            "pkg/__init__.py": "from toolr import command_group as cg\n",
        },
        ["a"],
    ),
    "relative import in a package": (
        {
            "pkg/__init__.py": "from .lib import cg\n" + GROUP.format("a"),
            "pkg/lib.py": "from toolr import command_group as cg\n",
        },
        ["a"],
    ),
    "relative import above the root": (
        {
            "m.py": "from .... import cg\n" + GROUP.format("a"),
        },
        [],
    ),
    "relative import to the root": (
        {
            "pkg/m.py": "from .. import cg\n" + GROUP.format("a"),
            "__init__.py": "from toolr import command_group as cg\n",
        },
        ["a"],
    ),
    "from a module outside tools": (
        {"m.py": "from elsewhere import cg\n" + GROUP.format("a")},
        [],
    ),
    "star import from toolr": (
        {"m.py": "from toolr import *\ncg = command_group\n" + GROUP.format("a")},
        ["a"],
    ),
    "star import alias from toolr": (
        {"m.py": "from toolr import *\n" + GROUP.format("a").replace("cg(", "command_group(")},
        ["a"],
    ),
    "star import of the member name from toolr": (
        {
            "m.py": "from toolr import *\nfrom tools.lib import cg\n" + GROUP.format("a"),
            "lib.py": "from toolr import *\ncg = command_group\n",
        },
        ["a"],
    ),
    "star import of another name from toolr": (
        {"m.py": "from toolr import *\n" + GROUP.format("a")},
        [],
    ),
    "star import from a toolr submodule": (
        {"m.py": "from toolr._decorators import *\n" + GROUP.format("a")},
        [],
    ),
    "star import from a tools module": (
        {
            "m.py": "from tools.lib import *\n" + GROUP.format("a"),
            "lib.py": "from toolr import command_group as cg\n",
        },
        ["a"],
    ),
    "star import from outside tools": (
        {"m.py": "from elsewhere import *\n" + GROUP.format("a")},
        [],
    ),
    "star import beside a stronger one": (
        {
            "m.py": "from elsewhere import *\nfrom tools.lib import *\n" + GROUP.format("a"),
            "lib.py": "from toolr import command_group as cg\n",
        },
        ["a"],
    ),
    "no star import at all": (
        {"m.py": GROUP.format("a")},
        [],
    ),
    "longest chain": (
        {
            **{f"c{i}.py": f"from tools.c{i + 1} import cg\n" for i in range(7)},
            "c7.py": "from toolr import command_group as cg\n",
            "m.py": "from tools.c0 import cg\n" + GROUP.format("a"),
        },
        ["a"],
    ),
    "chain one hop too long": (
        {
            **{f"c{i}.py": f"from tools.c{i + 1} import cg\n" for i in range(8)},
            "c8.py": "from toolr import command_group as cg\n",
            "m.py": "from tools.c0 import cg\n" + GROUP.format("a"),
        },
        [],
    ),
    "self copy resolves through the import": (
        {"m.py": "from toolr import command_group as cg\ncg = cg\n" + GROUP.format("a")},
        ["a"],
    ),
    "annotated self copy resolves through the import": (
        {"m.py": "from toolr import command_group as cg\ncg: object = cg\n" + GROUP.format("a")},
        ["a"],
    ),
    "self copy through a star import": (
        {
            "m.py": "from tools.lib import *\ncg = cg\n" + GROUP.format("a"),
            "lib.py": "from toolr import command_group as cg\n",
        },
        ["a"],
    ),
    "self copy of nothing": (
        {"m.py": "cg = cg\n" + GROUP.format("a")},
        [],
    ),
    "cycle": (
        {
            "x.py": "from tools.y import cg\n",
            "y.py": "from tools.x import cg\n",
            "m.py": "from tools.x import cg\n" + GROUP.format("a"),
        },
        [],
    ),
}


@pytest.mark.parametrize(("files", "expected"), ALIAS_CASES.values(), ids=ALIAS_CASES.keys())
def test_aliases_resolve_from_source_as_the_parser_does(
    make_tools_dir: Callable[..., Path], files: dict[str, str], expected: list[str]
) -> None:
    """Nothing is imported: the resolver reads the `tools/` modules' source only."""
    tools = make_tools_dir(files)
    assert [d.path for d in _group_declarations(tools)] == expected


def test_resolution_is_memoised() -> None:
    """Modules star-importing each other fan out wide per hop, up to the depth cap, without a memo."""
    count = 6
    names = [f"tools.m{n}" for n in range(count)]
    tables = {
        module: _ImportTable(
            ast.parse(
                "".join(f"from tools.m{o} import *\n" for o in range(count) if o != n)
                + f"def helper{n}():\n    pass\n"
            ),
            module,
            is_package=False,
        )
        for n, module in enumerate(names)
    }
    memo: _Memo = {}
    for module in names:
        for n in range(count):
            assert _binding(tables, module, f"helper{n}", 0, memo) is _Binding.OTHER
    # At most one entry per (module, name, depth in 0..=_MAX_REEXPORT_DEPTH).
    assert len(memo) <= len(names) * count * (_MAX_REEXPORT_DEPTH + 1)


def test_alias_of_a_package_attribute_resolves_through_its_init(
    make_tools_dir: Callable[..., Path],
) -> None:
    tools = make_tools_dir(
        {
            "m.py": "from tools.pkg import cg\n" + GROUP.format("a"),
            "pkg/__init__.py": "from toolr import command_group as cg\n",
        }
    )
    assert [d.path for d in _group_declarations(tools)] == ["a"]


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


IMPORT = "from toolr import command_group\n"


def _problems(module: str, *labels: str) -> list[tuple[str, str, str]]:
    return [(module, label, GROUP_BINDING) for label in labels]


def _unsupported(make_tools_dir: Callable[..., Path], source: str, **others: str) -> list:
    files = {"m.py": IMPORT + textwrap.dedent(source)}
    files.update({f"{name}.py": IMPORT + textwrap.dedent(body) for name, body in others.items()})
    return _unsupported_group_bindings(make_tools_dir(files))


def test_plain_bindings_are_not_unsupported(make_tools_dir: Callable[..., Path]) -> None:
    source = """
        a = command_group("a", "A", "A.")
        b: T = command_group("b", "B", "B.")
        command_group("c", "C", "C.")
        g = command_group("g", "G", "G.", parent=command_group("p", "P", "P."))
        h: Annotated[object, command_group("h", "H", "H.")] = command_group("h", "H", "H.")
    """
    assert _unsupported(make_tools_dir, source) == []


@pytest.mark.parametrize(
    "call",
    [
        'a.command_group("k", "K", "K.")',
        'make().command_group("k", "K", "K.", parent="ci")',
        'nope.command_group("k", "K", "K.")',
    ],
)
def test_the_method_form_is_an_unsupported_declaration(
    call: str, make_tools_dir: Callable[..., Path]
) -> None:
    source = f'a = command_group("a", "A", "A.")\nk = {call}\n'
    assert _unsupported(make_tools_dir, source) == [("tools.m", 'command_group("k")', METHOD_GROUP)]


def test_tuple_target_is_an_unsupported_declaration(make_tools_dir: Callable[..., Path]) -> None:
    source = 'a, b = command_group("a", "A", "A."), command_group("b", "B", "B.")\n'
    assert _unsupported(make_tools_dir, source) == _problems(
        "tools.m", 'command_group("a")', 'command_group("b")'
    )


def test_walrus_and_chained_target_are_unsupported_declarations(
    make_tools_dir: Callable[..., Path],
) -> None:
    source = """
        (g := command_group("g", "G", "G."))
        a = b = command_group("a", "A", "A.")
    """
    assert _unsupported(make_tools_dir, source) == _problems(
        "tools.m", 'command_group("g")', 'command_group("a")'
    )


@pytest.mark.parametrize(
    "head",
    [
        "if x:",
        "try:\n    pass\nfinally:",
        "with x:",
        "for _ in x:",
        "while x:",
        "def make():",
        "class C:",
    ],
)
def test_group_in_a_nested_body_is_an_unsupported_declaration(
    head: str, make_tools_dir: Callable[..., Path]
) -> None:
    source = f'{head}\n    g = command_group("g", "G", "G.")\n'
    assert _unsupported(make_tools_dir, source) == _problems("tools.m", 'command_group("g")')


def test_group_in_a_comprehension_or_lambda_is_an_unsupported_declaration(
    make_tools_dir: Callable[..., Path],
) -> None:
    source = """
        gs = [command_group(n, "T", "D.") for n in "ab"]
        make = lambda: command_group("l", "L", "L.")
    """
    assert _unsupported(make_tools_dir, source) == _problems(
        "tools.m", "command_group", 'command_group("l")'
    )


def test_bare_group_in_a_match_is_an_unsupported_declaration(
    make_tools_dir: Callable[..., Path],
) -> None:
    source = 'match x:\n    case 1:\n        command_group("g", "G", "G.")\n'
    assert _unsupported(make_tools_dir, source) == _problems("tools.m", 'command_group("g")')


def test_call_nested_in_an_expression_is_an_unsupported_declaration(
    make_tools_dir: Callable[..., Path],
) -> None:
    source = """
        command_group("g", "G", "G.").command(f)
        @command_group("d", "D", "D.")
        def f(): ...
    """
    assert _unsupported(make_tools_dir, source) == _problems(
        "tools.m", 'command_group("g")', 'command_group("d")'
    )


def test_aliased_and_attribute_calls_under_an_if_are_unsupported(
    make_tools_dir: Callable[..., Path],
) -> None:
    files = {
        "m.py": "from toolr import command_group as cg\nif x:\n    g = cg('g', 'G', 'G.')\n",
        "n.py": "import toolr as t\nif x:\n    g = t.command_group('n', 'N', 'N.')\n",
        "o.py": "import toolr.types\nif x:\n    g = toolr.command_group('o', 'O', 'O.')\n",
    }
    assert _unsupported_group_bindings(make_tools_dir(files)) == [
        ("tools.m", 'command_group("g")', GROUP_BINDING),
        ("tools.n", 'command_group("n")', GROUP_BINDING),
        ("tools.o", 'command_group("o")', GROUP_BINDING),
    ]


@pytest.mark.parametrize(
    "source",
    [
        "def command_group(*a):\n    return a\nif x:\n    h = command_group('a', 'b', 'c')\n",
        "def helper(command_group):\n    return command_group('a', 'b', 'c')\n",
        "f = lambda command_group: command_group('a', 'b', 'c')\n",
        "from pathlib import Path as command_group\nif x:\n    h = command_group('a', 'b', 'c')\n",
        "command_group = print\nif x:\n    command_group('a', 'b', 'c')\n",
        (
            "class R:\n    def go(self, o):\n        self.command_group('a', 'b', 'c')\n"
            "        o.command_group('a', 'b', 'c')\n"
        ),
        "import toolr\ndef f(toolr):\n    return toolr.command_group('a', 'b', 'c')\n",
        "import other as toolr\nif x:\n    toolr.command_group('a', 'b', 'c')\n",
    ],
)
def test_calls_that_are_not_toolrs_are_not_reported(
    source: str, make_tools_dir: Callable[..., Path]
) -> None:
    assert _unsupported_group_bindings(make_tools_dir({"m.py": source})) == []


def test_parameter_shadows_only_the_function_body(make_tools_dir: Callable[..., Path]) -> None:
    source = """
        def f(command_group, other=command_group("d", "D", "D.")):
            return command_group("x", "X", "X.")
    """
    assert _unsupported(make_tools_dir, source) == _problems("tools.m", 'command_group("d")')


def test_name_only_calls_are_reported_too(make_tools_dir: Callable[..., Path]) -> None:
    source = """
        g = command_group("g", "G", "G.")
        def f(extra):
            command_group("g")
            command_group("h", "H")
            command_group("i", title="I")
            command_group("j", "J", None)
            command_group("k", "K", **extra)
    """
    assert _unsupported(make_tools_dir, source) == _problems(
        "tools.m", *(f'command_group("{name}")' for name in "ghijk")
    )


def test_problems_come_in_source_order(make_tools_dir: Callable[..., Path]) -> None:
    source = """
        if x:
            d[command_group("a", "A", "A.")] = command_group("b", "B", "B.")
            y = [command_group("c", "C", "C.") if command_group("d", "D", "D.") else 0]
            f(x=command_group("e", "E", "E."), *[command_group("f", "F", "F.")])
    """
    assert _unsupported(make_tools_dir, source) == _problems(
        "tools.m", *(f'command_group("{name}")' for name in "abcdef")
    )


def test_a_very_long_expression_does_not_overflow_the_stack() -> None:
    # Built node by node: CPython's own parser recurses on a long expression, at a depth
    # that varies by version and platform, so parsing one would fail before the walk runs.
    tree = ast.parse(IMPORT + 'x = 1 + command_group("g", "G", "G.")\n')
    assign = tree.body[1]
    assert isinstance(assign, ast.Assign)
    assert isinstance(assign.value, ast.BinOp)
    chain: ast.expr = ast.Constant(value=1)
    for _ in range(3000):
        chain = ast.BinOp(left=chain, op=ast.Add(), right=ast.Constant(value=1))
    assign.value.left = chain
    tables = {"m": _ImportTable(tree, "m", is_package=False)}

    calls = _unbound_group_calls(tree, tables, "m", {})

    assert [call.lineno for call in calls] == [2]


def test_group_bindings_come_before_commands(make_tools_dir: Callable[..., Path]) -> None:
    tools = make_tools_dir(
        {
            "m.py": IMPORT
            + textwrap.dedent(
                """
                command_group("grp", "Grp", "Grp.")
                (g := command_group("w", "W", "W."))
                """
            ),
            "n.py": IMPORT + 'if True:\n    command_group("n", "N", "N.")\n',
        }
    )
    assert _unsupported_group_bindings(tools) == [
        *_problems("tools.m", 'command_group("w")'),
        *_problems("tools.n", 'command_group("n")'),
    ]


def test_keyword_name_declares_and_names_a_group(make_tools_dir: Callable[..., Path]) -> None:
    source = 'g = command_group(name="kw", title="Kw", description="D.")\n'
    tools = make_tools_dir({"m.py": IMPORT + source})
    assert [d.path for d in _group_declarations(tools)] == ["kw"]
    named = 'def f():\n    command_group(name="g", description="D.")\n'
    assert _unsupported(make_tools_dir, named) == _problems("tools.m", 'command_group("g")')


def test_non_literal_name_is_reported_generically(make_tools_dir: Callable[..., Path]) -> None:
    source = """
        N = "n"
        a = command_group(N, "T", "D.")
        command_group(name=N)
        b: T = command_group(*args)
        ok = command_group(name="ok", title="Ok", description="D.")
        def f(n):
            return command_group(n, "N", "N.")
    """
    assert _unsupported(make_tools_dir, source) == _problems("tools.m", *["command_group"] * 4)
