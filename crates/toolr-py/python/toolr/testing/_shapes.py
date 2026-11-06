"""The manifest build's unsupported-declarations error, ported for ``CommandsTester``.

Each helper names the Rust function it mirrors; change both together so
``CommandsTester`` and ``toolr`` keep giving the same message.
"""

from __future__ import annotations

import ast
import sys
from collections.abc import Iterator
from pathlib import Path
from typing import Any

import toolr
from toolr._decorators import CommandGroup
from toolr.testing._groups import _NAME
from toolr.testing._groups import _argument
from toolr.testing._groups import _import_tables
from toolr.testing._groups import _literal
from toolr.testing._groups import _Memo
from toolr.testing._groups import _unbound_group_calls
from toolr.testing._unknown_groups import _escape_control_chars

# Mirrors `UnsupportedCommandKind`'s `Display` in `crates/toolr-core/src/parser/commands.rs`.
NESTED = "`@command` must decorate a top-level `def`, which toolr reads without running it."
NON_LITERAL_GROUP = "`group=` must be a string literal."
NON_LITERAL_NAME = "the command name must be a string literal."
STACKED = "a function can carry only one command decorator."
GROUP_BINDING = (
    "`command_group(...)` must be a top-level `name = command_group(...)` assignment or a bare statement, "
    "which toolr reads without running it."
)

FunctionNode = ast.FunctionDef | ast.AsyncFunctionDef

# (module, function, problem)
Unsupported = tuple[str, str, str]


def _is_str_literal(node: ast.expr) -> bool:
    return isinstance(node, ast.Constant) and isinstance(node.value, str)


def _command_decorators(
    node: FunctionNode, namespace: dict[str, Any]
) -> list[tuple[ast.expr, bool]]:
    """Each decorator naming toolr's ``command`` or a group's, and whether it's standalone.

    Resolved against the module's globals, which is what the static parser's
    import tables approximate. Rust resolves it by import chain (`Binding` in
    `toolr-core/src/parser/commands.rs`): change both together.
    """
    found = []
    for decorator in node.decorator_list:
        target = decorator.func if isinstance(decorator, ast.Call) else decorator
        if isinstance(target, ast.Name) and namespace.get(target.id) is toolr.command:
            found.append((decorator, True))
        elif (
            isinstance(target, ast.Attribute)
            and target.attr == "command"
            and isinstance(target.value, ast.Name)
        ):
            owner = namespace.get(target.value.id)
            if owner is toolr:
                found.append((decorator, True))
            elif isinstance(owner, CommandGroup):
                found.append((decorator, False))
    return found


def _problems(node: FunctionNode, top_level: bool, namespace: dict[str, Any]) -> list[str]:
    """What's wrong with one command's declaration, in the order Rust reports it."""
    found = _command_decorators(node, namespace)
    if not found:
        # Like the parser, a nested `@g.command` counts only on a module-level group.
        return []
    if not top_level:
        return [NESTED]
    if len(found) > 1:
        return [STACKED]
    call, standalone = found[0]
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


def _function_defs(node: ast.AST, top_level: bool) -> Iterator[tuple[FunctionNode, bool]]:
    """Every ``def`` under ``node`` in source order, and whether it sits in the module body."""
    for child in ast.iter_child_nodes(node):
        if isinstance(child, FunctionNode):
            yield child, top_level
        yield from _function_defs(child, top_level=False)


def _group_call_label(call: ast.Call) -> str:
    """``command_group("<name>")`` when the name argument is a string literal, else ``command_group``."""
    name = _literal(_argument(call, _NAME))
    return "command_group" if name is None else f'command_group("{name}")'


def _unsupported_group_bindings(tools_dir: Path) -> list[Unsupported]:
    """Every ``command_group(...)`` call the manifest build can't bind, in build order.

    Mirrors ``crates/toolr-core/src/parser/groups.rs::detect_unsupported_group_bindings``,
    including how it names a call.
    """
    trees, modules, tables = _import_tables(tools_dir)
    memo: _Memo = {}
    return [
        (modules[file], _group_call_label(call), GROUP_BINDING)
        for file, tree in trees.items()
        for call in _unbound_group_calls(tree, tables, modules[file], memo)
    ]


def _unsupported_commands(tools_dir: Path) -> list[Unsupported]:
    """Every problem with the commands ``tools_dir`` declares, in build order.

    Group-binding problems come first, as in the manifest build.

    Mirrors ``crates/toolr-core/src/parser/commands.rs::detect_unsupported_commands``.
    Each imported ``tools`` module's source is read whether or not a ``def`` ran,
    so a command behind ``if TYPE_CHECKING:`` or in an uncalled function counts.
    """
    root = tools_dir.resolve()
    files: dict[Path, tuple[str, dict[str, Any]]] = {}
    for name, module in list(sys.modules.items()):
        file = getattr(module, "__file__", None)
        if file is None or (name != "tools" and not name.startswith("tools.")):
            continue
        path = Path(file).resolve()
        if path.is_relative_to(root):
            files.setdefault(path, (name, vars(module)))
    found: list[Unsupported] = _unsupported_group_bindings(tools_dir)
    for path in sorted(files, key=lambda path: path.relative_to(root).parts):
        name, namespace = files[path]
        for node, top_level in _function_defs(ast.parse(path.read_bytes()), top_level=True):
            found.extend(
                (name, node.name, problem) for problem in _problems(node, top_level, namespace)
            )
    return found


def _format_unsupported_commands(found: list[Unsupported]) -> str:
    """The whole error for ``found``.

    Mirrors ``crates/toolr-core/src/parser/build.rs::format_unsupported_commands``
    and the ``BuildError::UnsupportedCommands`` header.
    """
    lines = [
        f"  - {_escape_control_chars(module)}::{_escape_control_chars(function)}: {problem}"
        for module, function, problem in found
    ]
    return "\n".join([f"unsupported command declarations ({len(lines)}):", *lines])
