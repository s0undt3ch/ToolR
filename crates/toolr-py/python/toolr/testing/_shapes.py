"""The manifest build's unsupported-declarations error, ported for ``CommandsTester``.

Each helper names the Rust function it mirrors; change both together so
``CommandsTester`` and ``toolr`` keep giving the same message.
"""

from __future__ import annotations

import ast
import inspect
import sys
import types
from collections.abc import Callable
from collections.abc import Iterable
from pathlib import Path
from typing import Any

import toolr
from toolr._decorators import CommandGroup
from toolr.testing._unknown_groups import _escape_control_chars
from toolr.testing._unknown_groups import _source_order_key

# Mirrors `UnsupportedCommandKind`'s `Display` in `crates/toolr-core/src/parser/commands.rs`.
NESTED = "`@command` must decorate a top-level `def`, which toolr reads without running it."
NON_LITERAL_GROUP = "`group=` must be a string literal."
NON_LITERAL_NAME = "the command name must be a string literal."

FunctionNode = ast.FunctionDef | ast.AsyncFunctionDef


def _is_str_literal(node: ast.expr) -> bool:
    return isinstance(node, ast.Constant) and isinstance(node.value, str)


def _command_decorator(
    node: FunctionNode, namespace: dict[str, Any]
) -> tuple[ast.expr, bool] | None:
    """The first decorator naming toolr's ``command`` or a group's, and whether it's standalone.

    Resolved against the module's globals, which is what the static parser's
    import tables approximate.
    """
    for decorator in node.decorator_list:
        target = decorator.func if isinstance(decorator, ast.Call) else decorator
        if isinstance(target, ast.Name) and namespace.get(target.id) is toolr.command:
            return decorator, True
        if (
            isinstance(target, ast.Attribute)
            and target.attr == "command"
            and isinstance(target.value, ast.Name)
        ):
            owner = namespace.get(target.value.id)
            if owner is toolr:
                return decorator, True
            if isinstance(owner, CommandGroup):
                return decorator, False
    return None


def _problems(node: FunctionNode, top_level: bool, namespace: dict[str, Any]) -> list[str]:
    """What's wrong with one command's declaration, in the order Rust reports it."""
    found = _command_decorator(node, namespace)
    if found is None:
        # Like the parser, a nested `@g.command` counts only on a module-level group.
        return []
    if not top_level:
        return [NESTED]
    call, standalone = found
    if not isinstance(call, ast.Call):
        return []
    keywords = {keyword.arg: keyword.value for keyword in call.keywords if keyword.arg}
    problems = []
    if standalone and "group" in keywords and not _is_str_literal(keywords["group"]):
        problems.append(NON_LITERAL_GROUP)
    name_args = [*call.args[:1], *([keywords["name"]] if "name" in keywords else [])]
    if any(not _is_str_literal(arg) for arg in name_args):
        problems.append(NON_LITERAL_NAME)
    return problems


def _find_def(tree: ast.Module, func: Callable[..., Any]) -> tuple[FunctionNode, bool] | None:
    """The ``def`` that made ``func``, and whether it sits directly in the module body."""
    code = func.__code__
    for node in ast.walk(tree):
        if (
            isinstance(node, FunctionNode)
            and node.name == func.__name__
            and min([node.lineno, *(d.lineno for d in node.decorator_list)]) == code.co_firstlineno
        ):
            return node, node in tree.body
    return None


def _unsupported_commands(
    funcs: Iterable[Callable[..., Any]], tools_dir: Path
) -> list[tuple[Callable[..., Any], str]]:
    """Every problem with the commands ``tools_dir`` declares, in build order.

    Mirrors ``crates/toolr-core/src/parser/commands.rs::detect_unsupported_commands``.
    Functions defined outside ``tools_dir`` (a test body, say) aren't checked.
    """
    trees: dict[str, ast.Module] = {}
    seen: set[types.CodeType] = set()
    found: list[tuple[Callable[..., Any], str]] = []
    for registered in funcs:
        # A `functools.wraps` decorator under the command hides the `def` it wraps.
        func = inspect.unwrap(registered)
        code = func.__code__
        if code in seen or not Path(code.co_filename).is_relative_to(tools_dir):
            continue
        seen.add(code)
        if code.co_filename not in trees:
            trees[code.co_filename] = ast.parse(Path(code.co_filename).read_text())
        located = _find_def(trees[code.co_filename], func)
        if located is None:
            continue
        module = sys.modules.get(func.__module__)
        namespace = vars(module) if module is not None else {}
        found.extend((func, problem) for problem in _problems(*located, namespace))
    found.sort(
        key=lambda item: (*_source_order_key(item[0], tools_dir), item[0].__code__.co_firstlineno)
    )
    return found


def _format_unsupported_commands(found: list[tuple[Callable[..., Any], str]]) -> str:
    """The whole error for ``found``.

    Mirrors ``crates/toolr-core/src/parser/build.rs::format_unsupported_commands``
    and the ``BuildError::UnsupportedCommands`` header.
    """
    lines = [
        f"  - {_escape_control_chars(func.__module__)}::{_escape_control_chars(func.__name__)}: "
        f"{problem}"
        for func, problem in found
    ]
    return "\n".join([f"unsupported command declarations ({len(lines)}):", *lines])
