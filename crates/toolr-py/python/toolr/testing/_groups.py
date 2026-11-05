"""The manifest build's command-group errors, ported for ``CommandsTester``.

``command_group`` doesn't check, at import, that some declaration of a group
describes it, so these read the ``command_group(...)`` calls from source the
way the manifest build does. Each helper names the Rust function it mirrors;
change both together so ``CommandsTester`` and ``toolr`` keep giving the same
message.
"""

from __future__ import annotations

import ast
import os
from pathlib import Path
from typing import NamedTuple

from toolr.testing._unknown_groups import _escape_control_chars


def _is_pruned_dir(name: str) -> bool:
    """Whether the ``tools/`` walks skip a directory called ``name``.

    Mirrors ``is_pruned_dir`` in ``crates/toolr-core/src/hash.rs``; change
    both together.
    """
    return name.startswith(".") or name == "__pycache__"


class GroupDeclaration(NamedTuple):
    """One ``command_group(...)`` call, as the manifest build reads it."""

    path: str
    """The group's dotted path."""
    module: str
    """The module making the call."""
    describes: bool
    """Whether the call passes a non-empty description or docstring."""
    contradictory: bool
    """Whether the call surely passes a docstring with a description or long description."""
    unreadable: bool = False
    """Whether the call may pass a description or docstring the parser can't see."""


def _python_files(tools_dir: Path) -> list[Path]:
    """Every ``*.py`` under ``tools_dir``, sorted by path components.

    Mirrors ``crates/toolr-core/src/hash.rs::list_python_files``, minus the
    symlink check ``discover()`` has already made.
    """
    found: list[Path] = []
    for directory, dirnames, filenames in os.walk(tools_dir):
        dirnames[:] = [name for name in dirnames if not _is_pruned_dir(name)]
        found.extend(
            Path(directory, name)
            for name in filenames
            if name.endswith(".py") and Path(directory, name).is_file()
        )
    return sorted(found, key=lambda path: path.relative_to(tools_dir).parts)


def _module_path(tools_dir: Path, file: Path) -> str:
    """Mirrors ``crates/toolr-core/src/parser/build.rs::module_path_for_prefix``."""
    parts = list(file.relative_to(tools_dir).with_suffix("").parts)
    if parts and parts[-1] == "__init__":
        parts.pop()
    return ".".join([tools_dir.name, *parts])


def _module_docstring(tree: ast.Module) -> str | None:
    """The module's docstring, or ``None`` when it has none.

    Mirrors ``crates/toolr-core/src/parser/build.rs::module_docstring``.
    """
    first = tree.body[0] if tree.body else None
    if (
        isinstance(first, ast.Expr)
        and isinstance(first.value, ast.Constant)
        and isinstance(first.value.value, str)
    ):
        return first.value.value
    return None


def _binds_doc(node: ast.AST) -> bool:
    """Whether ``node`` binds ``__doc__`` outside a function or class body.

    Mirrors ``crates/toolr-core/src/parser/groups.rs::binds_doc``.
    """
    if isinstance(node, ast.Name):
        return node.id == "__doc__" and not isinstance(node.ctx, ast.Load)
    if isinstance(node, ast.alias):
        return (node.asname or node.name).split(".")[0] == "__doc__"
    if isinstance(node, ast.ExceptHandler) and node.name == "__doc__":
        return True
    if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
        return False
    return any(_binds_doc(child) for child in ast.iter_child_nodes(node))


class _Module(NamedTuple):
    """What a ``command_group`` call's arguments can refer to.

    Mirrors ``Source`` in ``crates/toolr-core/src/parser/groups.rs``.
    """

    docstring: str | None
    doc_rebound: bool
    """Whether the module binds ``__doc__``, which then reads as unknown."""


def _is_doc(node: ast.expr | None) -> bool:
    return isinstance(node, ast.Name) and node.id == "__doc__"


def _literal(node: ast.expr | None) -> str | None:
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return node.value
    return None


def _argument(call: ast.Call, keyword: str, position: int) -> ast.expr | None:
    """Mirrors ``crates/toolr-core/src/parser/groups.rs::argument``."""
    for kw in call.keywords:
        if kw.arg == keyword:
            return kw.value
    return call.args[position] if position < len(call.args) else None


def _parent_var(call: ast.Call) -> str | None:
    """``parent`` in ``parent.command_group(...)``, else a ``parent=<name>`` argument.

    Mirrors ``parent_var_name`` then ``parent_kwarg_var`` in
    ``crates/toolr-core/src/parser/groups.rs``.
    """
    func = call.func
    if isinstance(func, ast.Attribute) and isinstance(func.value, ast.Name):
        return func.value.id
    parent = _argument(call, "parent", 5)
    return parent.id if isinstance(parent, ast.Name) else None


def _surely_passes(node: ast.expr | None, module: _Module) -> bool:
    """Whether ``node`` is surely an argument other than ``None`` at runtime.

    A literal, or the ``__doc__`` of a module with a docstring. Mirrors
    ``Source::surely_passes`` in ``crates/toolr-core/src/parser/groups.rs``.
    """
    if _is_doc(node):
        return not module.doc_rebound and module.docstring is not None
    return isinstance(node, ast.Constant) and node.value is not None


def _read(node: ast.expr | None, module: _Module) -> str | None:
    """The text of an argument, ``""`` for none, and ``None`` when unreadable.

    ``__doc__`` is the module docstring, and one that is empty or
    whitespace-only has no opinion. Any expression but a string literal,
    ``None`` or ``__doc__`` is unreadable. Mirrors ``Source::read`` in
    ``crates/toolr-core/src/parser/groups.rs``.
    """
    if node is None or (isinstance(node, ast.Constant) and node.value is None):
        return ""
    if _is_doc(node):
        if module.doc_rebound:
            return None
        return module.docstring if module.docstring and module.docstring.strip() else ""
    return _literal(node)


def _describes(call: ast.Call, module: _Module) -> bool:
    """Whether the call passes a non-empty description or docstring, readable or not.

    An empty description, or an empty or whitespace-only docstring, leaves the
    group undescribed. Mirrors ``describes`` in
    ``crates/toolr-core/src/parser/groups.rs::parse_group_call``.
    """
    description = _read(_argument(call, "description", 2), module)
    docstring = _read(_argument(call, "docstring", 4), module)
    return description != "" or docstring is None or bool(docstring.strip())


def _contradictory(call: ast.Call, module: _Module) -> bool:
    """Whether the call surely passes a docstring with a description or long description.

    Mirrors ``contradictory`` in
    ``crates/toolr-core/src/parser/groups.rs::parse_group_call``.
    """
    return _surely_passes(_argument(call, "docstring", 4), module) and (
        _surely_passes(_argument(call, "description", 2), module)
        or _surely_passes(_argument(call, "long_description", 3), module)
    )


def _unreadable(call: ast.Call, module: _Module) -> bool:
    """Whether the call may pass a description or docstring the parser can't see.

    Through ``*``/``**`` unpacking, or as the ``__doc__`` of a module without a
    docstring. Mirrors ``unreadable`` in
    ``crates/toolr-core/src/parser/groups.rs::parse_group_call``.
    """
    if any(isinstance(arg, ast.Starred) for arg in call.args) or any(
        kw.arg is None for kw in call.keywords
    ):
        return True
    blank = not module.doc_rebound and not (module.docstring or "").strip()
    return blank and any(
        _is_doc(node)
        for node in (_argument(call, "description", 2), _argument(call, "docstring", 4))
    )


class _GroupCall(NamedTuple):
    var: str | None
    call: ast.Call


def _group_calls(tree: ast.Module) -> list[_GroupCall]:
    """Each top-level ``command_group(...)`` call, with its bound name.

    Mirrors the statement match in
    ``crates/toolr-core/src/parser/groups.rs::extract_groups`` and
    ``group_binding``.
    """
    found: list[_GroupCall] = []
    for stmt in tree.body:
        var: str | None = None
        value: ast.expr | None = None
        if isinstance(stmt, ast.Assign):
            if len(stmt.targets) == 1 and isinstance(stmt.targets[0], ast.Name):
                var, value = stmt.targets[0].id, stmt.value
        elif isinstance(stmt, ast.Expr):
            value = stmt.value
        if isinstance(value, ast.Call) and (
            (isinstance(value.func, ast.Name) and value.func.id == "command_group")
            or (isinstance(value.func, ast.Attribute) and value.func.attr == "command_group")
        ):
            found.append(_GroupCall(var, value))
    return found


def _group_declarations(tools_dir: Path) -> list[GroupDeclaration]:
    """Every ``command_group(...)`` call under ``tools_dir``, in build order.

    Mirrors ``crates/toolr-core/src/parser/groups.rs::extract_groups`` and its
    ``parse_group_call``, including how a parent variable resolves: this
    module's bindings first, then the last binding of that name in an earlier
    file.
    """
    declarations: list[GroupDeclaration] = []
    global_vars: dict[str, str] = {}
    for file in _python_files(tools_dir):
        tree = ast.parse(file.read_bytes())
        module = _module_path(tools_dir, file)
        source = _Module(_module_docstring(tree), _binds_doc(tree))
        local: list[tuple[str, str]] = []
        for var, call in _group_calls(tree):
            parent_var = _parent_var(call)
            parent = None
            if parent_var is not None:
                parent = next((path for name, path in local if name == parent_var), None)
                if parent is None:
                    parent = global_vars.get(parent_var)
            if parent is None:
                parent = _literal(_argument(call, "parent", 5))
            name = _literal(call.args[0] if call.args else None)
            if name is None:
                continue
            if "." in name:
                parent, _, name = name.rpartition(".")
            path = f"{parent}.{name}" if parent else name
            local.append((var or name, path))
            declarations.append(
                GroupDeclaration(
                    path=path,
                    module=module,
                    describes=_describes(call, source),
                    contradictory=_contradictory(call, source),
                    unreadable=_unreadable(call, source),
                )
            )
        global_vars.update(local)
    return declarations


def _format_contradictory_groups(declarations: list[GroupDeclaration]) -> str | None:
    """The error for calls passing a docstring with a description, if any.

    Mirrors ``crates/toolr-core/src/parser/build.rs::format_contradictory_groups``
    and the ``BuildError::ContradictoryGroups`` header.
    """
    lines = [
        f"  - `{_escape_control_chars(d.path)}` in {_escape_control_chars(d.module)}: pass "
        "`docstring=` or `description=`/`long_description=`, not both."
        for d in declarations
        if d.contradictory
    ]
    if not lines:
        return None
    return "\n".join([f"command groups with contradictory arguments ({len(lines)}):", *lines])


_HINT = "give one declaration a description or docstring."
_UNREADABLE_HINT = (
    "give one declaration a description or docstring the static parser can read: "
    "a string literal, or the `__doc__` of a module with a docstring."
)


def _format_undescribed_groups(declarations: list[GroupDeclaration]) -> str | None:
    """The error for groups no declaration describes, if any.

    Mirrors ``crates/toolr-core/src/parser/build.rs::format_undescribed_groups``
    and the ``BuildError::UndescribedGroups`` header.
    """
    modules: dict[str, list[str]] = {}
    described: set[str] = set()
    unreadable: set[str] = set()
    for d in declarations:
        places = modules.setdefault(d.path, [])
        if d.module not in places:
            places.append(d.module)
        if d.describes:
            described.add(d.path)
        if d.unreadable:
            unreadable.add(d.path)
    lines = [
        f"  - `{_escape_control_chars(path)}` (declared in "
        f"{', '.join(_escape_control_chars(m) for m in places)}): "
        + (_UNREADABLE_HINT if path in unreadable else _HINT)
        for path, places in modules.items()
        if path not in described
    ]
    if not lines:
        return None
    return "\n".join([f"command groups with no description ({len(lines)}):", *lines])
