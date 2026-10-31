"""Resolve a command's type hints one parameter at a time.

`typing.get_type_hints` is all-or-nothing: one annotation naming something
unbound at run time (typically an import kept under `if TYPE_CHECKING:`)
makes it raise, and the caller loses every hint. This module evaluates each
annotation on its own, and on a `NameError` imports the missing name from the
module's `TYPE_CHECKING` block before giving up on that one parameter.
"""

from __future__ import annotations

import ast
import importlib
import importlib.util
import inspect
import re
import sys
import weakref
from collections.abc import Callable
from dataclasses import dataclass
from types import ModuleType
from typing import Any
from typing import get_type_hints

from toolr._exc import SpecError

_UNKNOWN_ARG_KEYWORD = re.compile(r"arg\(\) got an unexpected keyword argument '(?P<keyword>\w+)'")


def unknown_arg_keyword_error(exc: TypeError, where: str) -> SpecError | None:
    """A clear error for stale code passing a keyword `arg()` no longer takes.

    The static build rejects such code, but a cached manifest or a plugin
    fragment built by an older toolr still reaches the runner.
    """
    match = _UNKNOWN_ARG_KEYWORD.match(str(exc))
    if match is None:
        return None
    msg = (
        f"{where}: `arg()` has no `{match['keyword']}` keyword. "
        "Run `toolr project manifest rebuild` to see the replacement; "
        "if the module ships in a plugin, rebuild the plugin against this toolr release."
    )
    return SpecError(msg)


@dataclass(frozen=True)
class ResolvedHints:
    """Evaluated hints per parameter, plus the parameters that stayed unresolved."""

    hints: dict[str, Any]
    unresolved: dict[str, Exception]


@dataclass(frozen=True)
class _Binding:
    module: str
    attr: str | None
    #: Unaliased `import a.b` / `import a.c` all bind `a`; import every one.
    submodules: tuple[str, ...] = ()


_TYPE_CHECKING_IMPORTS: weakref.WeakKeyDictionary[ModuleType, dict[str, _Binding]] = (
    weakref.WeakKeyDictionary()
)


def qualified_name(target: Callable[..., Any]) -> str:
    """`module.qualname` of ``target``, as used in warnings and errors."""
    return f"{getattr(target, '__module__', '?')}.{getattr(target, '__qualname__', '?')}"


def _annotation_error(exc: Exception, where: str, what: str) -> SpecError:
    if isinstance(exc, TypeError):
        error = unknown_arg_keyword_error(exc, where)
        if error is not None:
            return error
    return SpecError(f"can't evaluate {what} on {where}: {exc}")


def _raw_annotations(target: Callable[..., Any]) -> dict[str, Any]:
    if sys.version_info >= (3, 14):
        import annotationlib  # noqa: PLC0415

        # FORWARDREF keeps closure-bound names and turns unbound ones into
        # ForwardRefs, where VALUE would raise on the first of them.
        try:
            return dict(
                annotationlib.get_annotations(target, format=annotationlib.Format.FORWARDREF)
            )
        except Exception as exc:
            # A lazy annotation that fails even without names, such as `1 / 0`.
            raise _annotation_error(exc, qualified_name(target), "the annotations") from exc
    return dict(inspect.get_annotations(target))


def signature(target: Callable[..., Any]) -> inspect.Signature:
    """`inspect.signature` that doesn't evaluate lazy (3.14+) annotations eagerly."""
    if sys.version_info >= (3, 14):
        import annotationlib  # noqa: PLC0415

        try:
            return inspect.signature(target, annotation_format=annotationlib.Format.FORWARDREF)
        except Exception as exc:
            raise _annotation_error(exc, qualified_name(target), "the annotations") from exc
    return inspect.signature(target)


_TYPING_MODULES = frozenset({"typing", "typing_extensions"})


def _type_checking_guards(tree: ast.Module) -> tuple[set[str], set[str]]:
    """Names bound to `TYPE_CHECKING`, and names bound to a module that defines it."""
    flags = {"TYPE_CHECKING"}
    modules = {"typing"}
    for node in tree.body:
        if isinstance(node, ast.ImportFrom) and node.module in _TYPING_MODULES and not node.level:
            flags.update(
                alias.asname or alias.name for alias in node.names if alias.name == "TYPE_CHECKING"
            )
        elif isinstance(node, ast.Import):
            modules.update(
                alias.asname or alias.name for alias in node.names if alias.name in _TYPING_MODULES
            )
    return flags, modules


def _is_type_checking_test(node: ast.expr, flags: set[str], modules: set[str]) -> bool:
    if isinstance(node, ast.Name):
        return node.id in flags
    return (
        isinstance(node, ast.Attribute)
        and node.attr == "TYPE_CHECKING"
        and isinstance(node.value, ast.Name)
        and node.value.id in modules
    )


def _scan_type_checking_imports(module: ModuleType) -> dict[str, _Binding]:
    try:
        tree = ast.parse(inspect.getsource(module))
    except (OSError, TypeError, SyntaxError):
        return {}
    package = getattr(module, "__package__", None) or ""
    flags, modules = _type_checking_guards(tree)
    table: dict[str, _Binding] = {}
    for node in tree.body:
        if not (isinstance(node, ast.If) and _is_type_checking_test(node.test, flags, modules)):
            continue
        for stmt in node.body:
            if isinstance(stmt, ast.Import):
                for alias in stmt.names:
                    if alias.asname:
                        table[alias.asname] = _Binding(alias.name, None)
                    else:
                        top = alias.name.partition(".")[0]
                        # Merge only into an earlier bare `import top...`, not a `from x import top`.
                        previous = table.get(top)
                        submodules = (
                            previous.submodules if previous and previous.module == top else ()
                        )
                        if alias.name != top:
                            submodules = (*submodules, alias.name)
                        table[top] = _Binding(top, None, submodules)
            elif isinstance(stmt, ast.ImportFrom):
                name = "." * stmt.level + (stmt.module or "")
                try:
                    source = importlib.util.resolve_name(name, package) if stmt.level else name
                except ImportError:
                    continue
                for alias in stmt.names:
                    if alias.name != "*":
                        table[alias.asname or alias.name] = _Binding(source, alias.name)
    return table


def _type_checking_imports(module: ModuleType) -> dict[str, _Binding]:
    try:
        return _TYPE_CHECKING_IMPORTS[module]
    except KeyError:
        table = _TYPE_CHECKING_IMPORTS[module] = _scan_type_checking_imports(module)
        return table


def _import_binding(binding: _Binding) -> Any:
    """Return the object `binding` names, or raise ImportError/AttributeError."""
    module = importlib.import_module(binding.module)
    for submodule in binding.submodules:
        importlib.import_module(submodule)
    if binding.attr is not None:
        try:
            return getattr(module, binding.attr)
        except AttributeError:
            # `from pkg import submodule`
            return importlib.import_module(f"{binding.module}.{binding.attr}")
    return module


def _evaluate(annotation: Any, globalns: dict[str, Any], localns: dict[str, Any]) -> Any:
    # A one-entry holder keeps `get_type_hints`' handling of strings, ForwardRefs and `None`.
    def holder() -> None: ...

    holder.__annotations__ = {"value": annotation}
    return get_type_hints(holder, globalns=globalns, localns=localns, include_extras=False)["value"]


class _RescueFailedError(Exception):
    """Importing a name from the `TYPE_CHECKING` block raised ``error``."""

    def __init__(self, error: Exception) -> None:
        super().__init__(str(error))
        self.error = error


def _resolve_one(
    annotation: Any, module: ModuleType | None, globalns: dict[str, Any], local: dict[str, Any]
) -> Any:
    """Evaluate one annotation, importing missing names from the module's `TYPE_CHECKING` block."""
    tried: set[str] = set()
    while True:
        try:
            return _evaluate(annotation, globalns, local)
        except NameError as exc:
            missing = exc.name or ""
            binding = None
            if module is not None and missing not in tried:
                binding = _type_checking_imports(module).get(missing)
            if binding is None:
                raise
            tried.add(missing)
            try:
                local[missing] = _import_binding(binding)
            except Exception as error:  # noqa: BLE001 — any import failure leaves the parameter unresolved.
                # Raised inside `except NameError`, so `error.__context__` keeps the NameError.
                raise _RescueFailedError(error) from None


def resolve_hints(
    target: Callable[..., Any], localns: dict[str, Any] | None = None
) -> ResolvedHints:
    """Evaluate each parameter annotation of ``target`` on its own.

    ``NameError``/``AttributeError``, or a failed import from the module's
    `TYPE_CHECKING` block, leave that parameter in ``unresolved``. Any other
    failure raises :class:`SpecError` naming the parameter.
    """
    unwrapped = inspect.unwrap(target)
    where = qualified_name(target)
    module = sys.modules.get(getattr(unwrapped, "__module__", "") or "")
    globalns: dict[str, Any] = getattr(unwrapped, "__globals__", None) or (
        vars(module) if module is not None else {}
    )
    local: dict[str, Any] = dict(localns or {})

    hints: dict[str, Any] = {}
    unresolved: dict[str, Exception] = {}
    for name, annotation in _raw_annotations(target).items():
        if name == "return":
            continue
        try:
            hints[name] = _resolve_one(annotation, module, globalns, local)
        except (NameError, AttributeError) as exc:
            unresolved[name] = exc
        except _RescueFailedError as exc:
            unresolved[name] = exc.error
        except Exception as exc:
            raise _annotation_error(exc, where, f"the annotation of `{name}`") from exc
    return ResolvedHints(hints=hints, unresolved=unresolved)
