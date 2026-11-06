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
from enum import Enum
from enum import IntEnum
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


class _DocState(Enum):
    """What ``__doc__`` is when a ``command_group`` call reads it.

    Mirrors ``DocState`` in ``crates/toolr-core/src/parser/groups.rs``, whose
    ``Text`` is a ``str`` here; change both together.
    """

    REBOUND = "rebound"
    """The module binds ``__doc__``, so it may not be the docstring."""
    MISSING = "missing"
    """The module has no docstring: ``__doc__`` is ``None``."""
    BLANK = "blank"
    """The module docstring is empty or whitespace-only."""


class _Source(NamedTuple):
    """What a ``command_group`` call's arguments can refer to.

    Mirrors ``Source`` in ``crates/toolr-core/src/parser/groups.rs``; change
    both together.
    """

    doc: _DocState | str
    """The ``__doc__`` state, or the module docstring when it has text."""

    @classmethod
    def of(cls, tree: ast.Module) -> _Source:
        """Mirrors ``Source::new`` in ``crates/toolr-core/src/parser/groups.rs``."""
        docstring = _module_docstring(tree)
        if _binds_doc(tree):
            return cls(_DocState.REBOUND)
        if docstring is None:
            return cls(_DocState.MISSING)
        if not docstring.strip():
            return cls(_DocState.BLANK)
        return cls(docstring)


class _Field(Enum):
    """A ``command_group`` field the parser has no text for.

    Mirrors ``Field`` in ``crates/toolr-core/src/parser/groups.rs``, whose
    ``Text`` is a non-empty ``str`` here; change both together.
    """

    EMPTY = "empty"
    """Not passed, ``None``, or empty: no opinion."""
    OPAQUE = "opaque"
    """Passed as any other expression: an opinion the build can't read."""


class _Passes(Enum):
    """Whether an argument is something other than ``None`` at runtime.

    Mirrors ``Passes`` in ``crates/toolr-core/src/parser/groups.rs``; change
    both together.
    """

    NEVER = "never"
    MAYBE = "maybe"
    """Unless an expression the parser can't read is ``None``."""
    SURELY = "surely"


class _Param(NamedTuple):
    """A ``command_group`` parameter: its keyword and position.

    The position is in ``command_group(name, title, description,
    long_description, docstring, parent)``, the signature of ``command_group``
    in ``toolr/_decorators.py``. Mirrors ``Param`` in
    ``crates/toolr-core/src/parser/groups.rs``; change both together.
    """

    keyword: str
    position: int


_DESCRIPTION = _Param("description", 2)
_LONG_DESCRIPTION = _Param("long_description", 3)
_DOCSTRING = _Param("docstring", 4)
_PARENT = _Param("parent", 5)


def _is_doc(node: ast.expr | None) -> bool:
    return isinstance(node, ast.Name) and node.id == "__doc__"


def _literal(node: ast.expr | None) -> str | None:
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return node.value
    return None


def _argument(call: ast.Call, param: _Param) -> ast.expr | None:
    """Mirrors ``crates/toolr-core/src/parser/groups.rs::argument``."""
    for kw in call.keywords:
        if kw.arg == param.keyword:
            return kw.value
    return call.args[param.position] if param.position < len(call.args) else None


def _parent_var(call: ast.Call) -> str | None:
    """``parent`` in ``parent.command_group(...)``, else a ``parent=<name>`` argument.

    Mirrors ``parent_var_name`` then ``parent_kwarg_var`` in
    ``crates/toolr-core/src/parser/groups.rs``.
    """
    func = call.func
    if isinstance(func, ast.Attribute) and isinstance(func.value, ast.Name):
        return func.value.id
    parent = _argument(call, _PARENT)
    return parent.id if isinstance(parent, ast.Name) else None


def _passes(node: ast.expr | None, source: _Source) -> _Passes:
    """Whether ``node`` is an argument other than ``None`` at runtime.

    A literal surely is, and the ``__doc__`` of a module with a docstring.
    Mirrors ``Source::passes`` in ``crates/toolr-core/src/parser/groups.rs``.
    """
    if node is None or (isinstance(node, ast.Constant) and node.value is None):
        return _Passes.NEVER
    if _is_doc(node):
        if source.doc is _DocState.MISSING:
            return _Passes.NEVER
        return _Passes.MAYBE if source.doc is _DocState.REBOUND else _Passes.SURELY
    return _Passes.SURELY if isinstance(node, ast.Constant) else _Passes.MAYBE


def _read(node: ast.expr | None, source: _Source) -> _Field | str:
    """One argument's field: its non-empty text, else a ``_Field``.

    ``__doc__`` is the module docstring, and one that is empty or
    whitespace-only has no opinion. Any expression but a string literal,
    ``None`` or ``__doc__`` is opaque. Mirrors ``Source::read`` in
    ``crates/toolr-core/src/parser/groups.rs``.
    """
    if node is None or (isinstance(node, ast.Constant) and node.value is None):
        return _Field.EMPTY
    if _is_doc(node):
        if source.doc is _DocState.REBOUND:
            return _Field.OPAQUE
        return source.doc if isinstance(source.doc, str) else _Field.EMPTY
    text = _literal(node)
    if text is None:
        return _Field.OPAQUE
    return text or _Field.EMPTY


def _describes(call: ast.Call, source: _Source) -> bool:
    """Whether the call passes a non-empty description or docstring, readable or not.

    An empty description, or an empty or whitespace-only docstring, leaves the
    group undescribed. Mirrors ``describes`` in
    ``crates/toolr-core/src/parser/groups.rs``.
    """
    if _read(_argument(call, _DESCRIPTION), source) is not _Field.EMPTY:
        return True
    docstring = _read(_argument(call, _DOCSTRING), source)
    return docstring is _Field.OPAQUE or (isinstance(docstring, str) and bool(docstring.strip()))


def _contradictory(call: ast.Call, source: _Source) -> bool:
    """Whether the call surely passes a docstring with a description or long description.

    Mirrors ``contradictory`` in ``crates/toolr-core/src/parser/groups.rs``.
    """

    def surely(param: _Param) -> bool:
        return _passes(_argument(call, param), source) is _Passes.SURELY

    return surely(_DOCSTRING) and (surely(_DESCRIPTION) or surely(_LONG_DESCRIPTION))


def _unreadable(call: ast.Call, source: _Source) -> bool:
    """Whether the call may pass a description or docstring the parser can't see.

    Through ``*``/``**`` unpacking, or as the ``__doc__`` of a module without a
    docstring. Mirrors ``unreadable`` in
    ``crates/toolr-core/src/parser/groups.rs``.
    """
    if any(isinstance(arg, ast.Starred) for arg in call.args) or any(
        kw.arg is None for kw in call.keywords
    ):
        return True
    blank = source.doc in (_DocState.MISSING, _DocState.BLANK)
    return blank and any(_is_doc(_argument(call, param)) for param in (_DESCRIPTION, _DOCSTRING))


class _GroupCall(NamedTuple):
    var: str | None
    call: ast.Call


class _Binding(IntEnum):
    """What a name is, when looking for a toolr member.

    Mirrors ``Binding`` in ``crates/toolr-core/src/parser/commands.rs``; the
    variants run weakest to strongest, as there, so ``max`` picks the strongest
    of several candidates. Change both together.
    """

    OTHER = 0
    """A local ``def``, another name, or a cycle."""
    UNKNOWN = 1
    """A module the parser can't read (outside ``tools/``), or a star import from a ``toolr.*`` submodule."""
    TOOLR = 2
    """toolr's member."""


_MAX_REEXPORT_DEPTH = 8
"""Re-export chains deeper than this are treated as cycles.

Mirrors ``MAX_REEXPORT_DEPTH`` in ``crates/toolr-core/src/parser/commands.rs``.
"""


class _Assigned(NamedTuple):
    """The right-hand side of ``name = <value>`` or ``name: T = <value>``.

    Mirrors ``Assigned`` in ``crates/toolr-core/src/parser/symbols.rs``: a plain
    name (``base`` is ``None``) or ``base.attr``.
    """

    base: str | None
    attr: str

    @classmethod
    def of(cls, node: ast.expr | None) -> _Assigned | None:
        """Mirrors ``Assigned::from_expr`` in ``crates/toolr-core/src/parser/symbols.rs``."""
        if isinstance(node, ast.Name):
            return cls(None, node.id)
        if isinstance(node, ast.Attribute) and isinstance(node.value, ast.Name):
            return cls(node.value.id, node.attr)
        return None


class _Imported(NamedTuple):
    """One ``from <module> import <original> [as <local>]``, resolved to an absolute module."""

    module: str
    original: str


def _resolve_relative_module(
    level: int, module: str | None, current: str, *, is_package: bool
) -> str | None:
    """Mirrors ``resolve_relative_module`` in ``crates/toolr-core/src/parser/symbols.rs``."""
    pops = max(level - 1, 0) if is_package else level
    segments = current.split(".")
    if pops > len(segments):
        return None
    out = ".".join(segments[: len(segments) - pops])
    if module:
        out = f"{out}.{module}" if out else module
    return out or None


def _is_type_checking_test(node: ast.expr) -> bool:
    """Mirrors ``is_type_checking_test`` in ``crates/toolr-core/src/parser/symbols.rs``."""
    if isinstance(node, ast.Name):
        return node.id == "TYPE_CHECKING"
    return (
        isinstance(node, ast.Attribute)
        and node.attr == "TYPE_CHECKING"
        and isinstance(node.value, ast.Name)
        and node.value.id == "typing"
    )


class _ImportTable:
    """What a module's top-level imports and bindings make each name.

    Mirrors the parts of ``ImportTable`` in
    ``crates/toolr-core/src/parser/symbols.rs`` that ``_binding`` reads, and
    records the same statements: ``from`` imports, bare imports, ``def``,
    ``class`` and (annotated) assignments, inside the top level, a
    ``TYPE_CHECKING`` block, or a ``try`` body or handler. Nothing else
    counts, so ``del`` and an ``if`` don't unbind or bind. Change both
    together.
    """

    def __init__(self, tree: ast.Module, module: str, *, is_package: bool) -> None:
        self.entries: dict[str, list[_Imported]] = {}
        self.star_modules: list[str] = []
        self.module_bindings: dict[str, str] = {}
        self._local_names: dict[str, int] = {}
        self._import_positions: dict[str, int] = {}
        self._assigned: dict[str, _Assigned] = {}
        self._seq = 0
        for stmt in tree.body:
            self._collect(stmt, module, is_package=is_package)

    def _collect(self, stmt: ast.stmt, module: str, *, is_package: bool) -> None:
        """Mirrors ``ImportTable::collect_stmt``."""
        if isinstance(stmt, ast.ImportFrom):
            self._collect_import_from(stmt, module, is_package=is_package)
        elif isinstance(stmt, ast.Import):
            for alias in stmt.names:
                local = alias.asname or alias.name.split(".")[0]
                self.module_bindings[local] = alias.name
        elif isinstance(stmt, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            self._bind_local(stmt.name)
        elif isinstance(stmt, ast.Assign):
            for target in stmt.targets:
                if isinstance(target, ast.Name):
                    self._bind_local(target.id)
                    assigned = _Assigned.of(stmt.value)
                    if assigned is not None:
                        self._assigned[target.id] = assigned
        elif isinstance(stmt, ast.AnnAssign):
            # A bare `name: T` binds nothing.
            if isinstance(stmt.target, ast.Name) and stmt.value is not None:
                self._bind_local(stmt.target.id)
                assigned = _Assigned.of(stmt.value)
                if assigned is not None:
                    self._assigned[stmt.target.id] = assigned
        elif isinstance(stmt, ast.If) and _is_type_checking_test(stmt.test):
            # Not `orelse`: that is the runtime branch.
            for inner in stmt.body:
                self._collect(inner, module, is_package=is_package)
        elif isinstance(stmt, (ast.Try, ast.TryStar)):
            # Not `orelse` or `finalbody`.
            for inner in stmt.body:
                self._collect(inner, module, is_package=is_package)
            for handler in stmt.handlers:
                for inner in handler.body:
                    self._collect(inner, module, is_package=is_package)

    def _collect_import_from(self, stmt: ast.ImportFrom, module: str, *, is_package: bool) -> None:
        """Mirrors ``ImportTable::collect_import_from``."""
        if stmt.level == 0:
            target = stmt.module
        else:
            target = _resolve_relative_module(
                stmt.level, stmt.module, module, is_package=is_package
            )
        if target is None:
            return
        for alias in stmt.names:
            if alias.name == "*":
                self.star_modules.append(target)
                continue
            local = alias.asname or alias.name
            self._seq += 1
            self._import_positions[local] = self._seq
            self.entries.setdefault(local, []).append(_Imported(target, alias.name))

    def _bind_local(self, name: str) -> None:
        self._seq += 1
        self._assigned.pop(name, None)
        self._local_names[name] = self._seq

    def local_binding_wins(self, name: str) -> bool:
        """Whether the last top-level binding of ``name`` is the module's own, not an import."""
        local = self._local_names.get(name)
        if local is None:
            return False
        return local > self._import_positions.get(name, 0)

    def assigned(self, name: str) -> _Assigned | None:
        """What the last top-level binding of ``name`` copied, if it is a copy."""
        return self._assigned.get(name) if self.local_binding_wins(name) else None


def _is_toolr_path(module: str) -> bool:
    return module == "toolr" or module.startswith("toolr.")


def _toolr_member(attr: str, member: str) -> _Binding:
    return _Binding.TOOLR if attr == member else _Binding.OTHER


_Memo = dict[tuple[str, str, str, int], _Binding]
"""Results of ``_binding`` keyed by ``(module, name, member, depth)``.

The depth is part of the key because the depth cap truncates a walk, so a
result is only reusable at the same depth. Mirrors ``BindingCache`` in
``crates/toolr-core/src/parser/commands.rs``.
"""


def _assigned_binding(
    tables: dict[str, _ImportTable], module: str, name: str, member: str, depth: int, memo: _Memo
) -> _Binding:
    """What ``name``, bound by ``module`` itself, is: only a copy of another name or attribute can be toolr's."""
    assigned = tables[module].assigned(name)
    if assigned is None:
        return _Binding.OTHER
    if assigned.base is None:
        if assigned.attr == name:
            # `cg = cg` copies whatever the name was before: the imports.
            return _imported_binding(tables, module, name, member, depth, memo)
        return _binding(tables, module, assigned.attr, member, memo, depth + 1)
    bound = tables[module].module_bindings.get(assigned.base)
    if bound is None:
        return _Binding.OTHER
    if _is_toolr_path(bound):
        # `import toolr.types` binds `toolr`, not `toolr.types`.
        return _toolr_member(assigned.attr, member)
    return _binding(tables, bound, assigned.attr, member, memo, depth + 1)


def _imported_binding(
    tables: dict[str, _ImportTable], module: str, name: str, member: str, depth: int, memo: _Memo
) -> _Binding:
    """What ``name`` is when read from ``module``'s ``from ... import`` statements, ignoring a local binding."""
    table = tables[module]

    def follows(source: str, original: str) -> _Binding:
        if _is_toolr_path(source):
            return _toolr_member(original, member)
        return _binding(tables, source, original, member, memo, depth + 1)

    candidates = table.entries.get(name, [])
    if not candidates:
        # `from toolr.x import *` may or may not carry `member`.
        found = _Binding.OTHER
        for source in table.star_modules:
            if source == "toolr":
                found = max(found, follows(source, name))
            elif _is_toolr_path(source):
                found = max(found, _Binding.UNKNOWN)
            else:
                found = max(found, _binding(tables, source, name, member, memo, depth + 1))
        return found
    return max((follows(c.module, c.original) for c in candidates), default=_Binding.OTHER)


def _binding(
    tables: dict[str, _ImportTable],
    module: str,
    name: str,
    member: str,
    memo: _Memo,
    depth: int = 0,
) -> _Binding:
    """Follow ``name`` in ``module`` through imports, re-exports and assignments.

    Decides whether it is toolr's ``member``, from the ``tools/`` modules' source
    alone. Mirrors ``binds_toolr_member`` in
    ``crates/toolr-core/src/parser/commands.rs``; change both together.
    """
    if depth > _MAX_REEXPORT_DEPTH:
        return _Binding.OTHER
    key = (module, name, member, depth)
    hit = memo.get(key)
    if hit is not None:
        return hit
    table = tables.get(module)
    if table is None:
        found = _Binding.UNKNOWN
    elif table.local_binding_wins(name):
        found = _assigned_binding(tables, module, name, member, depth, memo)
    else:
        found = _imported_binding(tables, module, name, member, depth, memo)
    memo[key] = found
    return found


def _is_command_group(
    func: ast.expr, tables: dict[str, _ImportTable], module: str, memo: _Memo
) -> bool:
    """Whether ``func`` is toolr's ``command_group``.

    A bare name counts when spelt ``command_group`` or resolved to it through
    the ``tools/`` modules' imports; any ``<x>.command_group`` counts. Mirrors
    ``crates/toolr-core/src/parser/groups.rs::is_command_group_call``: change
    both together.
    """
    if isinstance(func, ast.Name):
        return (
            func.id == "command_group"
            or _binding(tables, module, func.id, "command_group", memo) is _Binding.TOOLR
        )
    return isinstance(func, ast.Attribute) and func.attr == "command_group"


def _group_calls(
    tree: ast.Module, tables: dict[str, _ImportTable], module: str, memo: _Memo
) -> list[_GroupCall]:
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
        elif isinstance(stmt, ast.AnnAssign):
            if isinstance(stmt.target, ast.Name):
                var, value = stmt.target.id, stmt.value
        elif isinstance(stmt, ast.Expr):
            value = stmt.value
        if isinstance(value, ast.Call) and _is_command_group(value.func, tables, module, memo):
            found.append(_GroupCall(var, value))
    return found


def _group_declarations(tools_dir: Path) -> list[GroupDeclaration]:
    """Every ``command_group(...)`` call under ``tools_dir``, in build order.

    Mirrors ``crates/toolr-core/src/parser/groups.rs::extract_groups`` and its
    ``parse_group_call``, including how a parent variable resolves: this
    module's bindings first, then the last binding of that name in an earlier
    file. An alias of ``command_group`` resolves from the modules' source, so
    nothing needs importing first.
    """
    declarations: list[GroupDeclaration] = []
    global_vars: dict[str, str] = {}
    files = _python_files(tools_dir)
    trees = {file: ast.parse(file.read_bytes()) for file in files}
    modules = {file: _module_path(tools_dir, file) for file in files}
    tables = {
        modules[file]: _ImportTable(trees[file], modules[file], is_package=file.stem == "__init__")
        for file in files
    }
    memo: _Memo = {}
    for file in files:
        tree = trees[file]
        module = modules[file]
        source = _Source.of(tree)
        local: list[tuple[str, str]] = []
        for var, call in _group_calls(tree, tables, module, memo):
            parent_var = _parent_var(call)
            parent = None
            if parent_var is not None:
                parent = next((path for name, path in local if name == parent_var), None)
                if parent is None:
                    parent = global_vars.get(parent_var)
            if parent is None:
                parent = _literal(_argument(call, _PARENT))
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


class _UndescribedGroup(NamedTuple):
    """A group none of whose declarations passes a description or docstring.

    Mirrors ``UndescribedGroup`` in ``crates/toolr-core/src/parser/build.rs``.
    """

    path: str
    """The group's dotted path."""
    modules: list[str]
    """Every declaring module, in build order."""
    unreadable: bool
    """Whether a declaration may pass a description or docstring the parser can't see."""


def _undescribed_groups(declarations: list[GroupDeclaration]) -> list[_UndescribedGroup]:
    """Every group no declaration describes, in first-declaration order.

    Mirrors ``crates/toolr-core/src/parser/build.rs::undescribed_groups``.
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
    return [
        _UndescribedGroup(path, places, unreadable=path in unreadable)
        for path, places in modules.items()
        if path not in described
    ]


def _format_undescribed_groups(groups: list[_UndescribedGroup]) -> str | None:
    """The error for groups no declaration describes, if any.

    Mirrors ``crates/toolr-core/src/parser/build.rs::format_undescribed_groups``
    and the ``BuildError::UndescribedGroups`` header.
    """
    lines = [
        f"  - `{_escape_control_chars(g.path)}` (declared in "
        f"{', '.join(_escape_control_chars(m) for m in g.modules)}): "
        + (_UNREADABLE_HINT if g.unreadable else _HINT)
        for g in groups
    ]
    if not lines:
        return None
    return "\n".join([f"command groups with no description ({len(lines)}):", *lines])
