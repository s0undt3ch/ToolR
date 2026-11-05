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
    """Whether the call passes a description or docstring."""
    contradictory: bool
    """Whether the call passes a docstring with a description or long description."""


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


def _module_docstring(tree: ast.Module) -> str:
    """Mirrors ``crates/toolr-core/src/parser/build.rs::module_docstring``."""
    first = tree.body[0] if tree.body else None
    if (
        isinstance(first, ast.Expr)
        and isinstance(first.value, ast.Constant)
        and isinstance(first.value.value, str)
    ):
        return first.value.value
    return ""


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
    """``parent`` in ``parent.command_group(...)``, else a ``parent=<name>`` keyword.

    Mirrors ``parent_var_name`` then ``parent_kwarg_var`` in
    ``crates/toolr-core/src/parser/groups.rs``.
    """
    func = call.func
    if isinstance(func, ast.Attribute) and isinstance(func.value, ast.Name):
        return func.value.id
    parent = next((kw.value for kw in call.keywords if kw.arg == "parent"), None)
    return parent.id if isinstance(parent, ast.Name) else None


def _passed(node: ast.expr | None, module_docstring: str) -> bool:
    """Whether ``node`` is an argument at all, as ``command_group`` sees it.

    ``None`` is no argument, and ``__doc__`` is ``None`` in a module without a
    docstring. Mirrors ``passed`` in
    ``crates/toolr-core/src/parser/groups.rs::parse_group_call``.
    """
    if node is None or (isinstance(node, ast.Constant) and node.value is None):
        return False
    if isinstance(node, ast.Name) and node.id == "__doc__":
        return bool(module_docstring)
    return True


def _group_calls(tree: ast.Module) -> list[tuple[str | None, ast.Call]]:
    """``(bound name, call)`` for each top-level ``command_group(...)`` call.

    Mirrors the statement match in
    ``crates/toolr-core/src/parser/groups.rs::extract_groups``.
    """
    found: list[tuple[str | None, ast.Call]] = []
    for stmt in tree.body:
        if isinstance(stmt, ast.Assign):
            if len(stmt.targets) != 1 or not isinstance(stmt.targets[0], ast.Name):
                continue
            var: str | None = stmt.targets[0].id
            value = stmt.value
        elif isinstance(stmt, ast.Expr):
            var, value = None, stmt.value
        else:
            continue
        if not isinstance(value, ast.Call):
            continue
        func = value.func
        if (isinstance(func, ast.Name) and func.id == "command_group") or (
            isinstance(func, ast.Attribute) and func.attr == "command_group"
        ):
            found.append((var, value))
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
        tree = ast.parse(file.read_text())
        module = _module_path(tools_dir, file)
        docstring = _module_docstring(tree)
        local: list[tuple[str, str]] = []
        for var, call in _group_calls(tree):
            parent_var = _parent_var(call)
            parent = None
            if parent_var is not None:
                parent = next((path for name, path in local if name == parent_var), None)
                if parent is None:
                    parent = global_vars.get(parent_var)
            if parent is None:
                parent = _literal(
                    next((kw.value for kw in call.keywords if kw.arg == "parent"), None)
                )
            name = _literal(call.args[0] if call.args else None)
            if name is None:
                continue
            if "." in name:
                parent, _, name = name.rpartition(".")
            path = f"{parent}.{name}" if parent else name
            local.append((var or name, path))
            docstring_arg = _passed(_argument(call, "docstring", 4), docstring)
            description_arg = _passed(_argument(call, "description", 2), docstring)
            declarations.append(
                GroupDeclaration(
                    path=path,
                    module=module,
                    describes=docstring_arg or description_arg,
                    contradictory=docstring_arg
                    and (
                        description_arg
                        or _passed(_argument(call, "long_description", 3), docstring)
                    ),
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


def _format_undescribed_groups(declarations: list[GroupDeclaration]) -> str | None:
    """The error for groups no declaration describes, if any.

    Mirrors ``crates/toolr-core/src/parser/build.rs::format_undescribed_groups``
    and the ``BuildError::UndescribedGroups`` header.
    """
    modules: dict[str, list[str]] = {}
    described: set[str] = set()
    for d in declarations:
        places = modules.setdefault(d.path, [])
        if d.module not in places:
            places.append(d.module)
        if d.describes:
            described.add(d.path)
    lines = [
        f"  - `{_escape_control_chars(path)}` (declared in "
        f"{', '.join(_escape_control_chars(m) for m in places)}): give one declaration a "
        "description or docstring."
        for path, places in modules.items()
        if path not in described
    ]
    if not lines:
        return None
    return "\n".join([f"command groups with no description ({len(lines)}):", *lines])
