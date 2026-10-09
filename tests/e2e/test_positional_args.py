"""Positional-argument shapes, driven through the real `toolr` binary (#539)."""

from __future__ import annotations

from collections.abc import Callable

from tests.conftest import ToolsProject

EXAMPLE_MODULE = '''
import enum
import pathlib

from toolr import Context
from toolr import command_group

example = command_group("example", "Example", description="Example commands")


class Foo(enum.Enum):
    A = "a"
    B = "b"


@example.command
def positional(ctx: Context, foo: Foo, *paths: pathlib.Path) -> None:
    """Demonstrate positional arguments.

    Args:
        foo: An example enum value.
        paths: A variable number of file paths.
    """
    ctx.print(f"foo: {foo}")
    ctx.print(f"paths: {[p.as_posix() for p in paths]}")


@example.command
def counted(ctx: Context, name: str, /, count: int, *rest: int) -> None:
    """Demonstrate positional-only arguments.

    Args:
        name: A positional-only name.
        count: A count.
        rest: More counts.
    """
    ctx.print(f"name={name!r} count={count!r} rest={rest!r}")
'''


def test_positional_before_variadic(make_tools_project: Callable[..., ToolsProject]) -> None:
    project = make_tools_project(example=EXAMPLE_MODULE)
    result = project.run("example", "positional", "a", "tools/env/setup.py.bak", "tools/example.py")
    assert result.returncode == 0, result.stderr
    assert "foo: Foo.A" in result.stdout
    assert "paths: ['tools/env/setup.py.bak', 'tools/example.py']" in result.stdout


def test_positional_only_before_variadic(make_tools_project: Callable[..., ToolsProject]) -> None:
    project = make_tools_project(example=EXAMPLE_MODULE)
    result = project.run("example", "counted", "x", "3", "4", "5")
    assert result.returncode == 0, result.stderr
    assert "name='x' count=3 rest=(4, 5)" in result.stdout
