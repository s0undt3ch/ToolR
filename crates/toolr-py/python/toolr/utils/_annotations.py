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


_TYPE_CHECKING_IMPORTS: weakref.WeakKeyDictionary[ModuleType, dict[str, _Binding]] = (
    weakref.WeakKeyDictionary()
)


def _raw_annotations(target: Callable[..., Any]) -> dict[str, Any]:
    if sys.version_info >= (3, 14):
        import annotationlib  # noqa: PLC0415

        # FORWARDREF keeps closure-bound names and turns unbound ones into
        # ForwardRefs, where VALUE would raise on the first of them.
        return dict(annotationlib.get_annotations(target, format=annotationlib.Format.FORWARDREF))
    return dict(inspect.get_annotations(target))


def signature(target: Callable[..., Any]) -> inspect.Signature:
    """`inspect.signature` that doesn't evaluate lazy (3.14+) annotations eagerly."""
    if sys.version_info >= (3, 14):
        import annotationlib  # noqa: PLC0415

        return inspect.signature(target, annotation_format=annotationlib.Format.FORWARDREF)
    return inspect.signature(target)


def _is_type_checking_test(node: ast.expr) -> bool:
    if isinstance(node, ast.Name):
        return node.id == "TYPE_CHECKING"
    return (
        isinstance(node, ast.Attribute)
        and node.attr == "TYPE_CHECKING"
        and isinstance(node.value, ast.Name)
        and node.value.id == "typing"
    )


def _scan_type_checking_imports(module: ModuleType) -> dict[str, _Binding]:
    try:
        tree = ast.parse(inspect.getsource(module))
    except (OSError, TypeError, SyntaxError):
        return {}
    package = getattr(module, "__package__", None) or ""
    table: dict[str, _Binding] = {}
    for node in tree.body:
        if not (isinstance(node, ast.If) and _is_type_checking_test(node.test)):
            continue
        for stmt in node.body:
            if isinstance(stmt, ast.Import):
                for alias in stmt.names:
                    if alias.asname:
                        table[alias.asname] = _Binding(alias.name, None)
                    else:
                        top = alias.name.partition(".")[0]
                        table[top] = _Binding(alias.name, None)
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


def _import_binding(name: str, binding: _Binding) -> Any:
    """Return the object `binding` names, or raise ImportError/AttributeError."""
    module = importlib.import_module(binding.module)
    if binding.attr is not None:
        try:
            return getattr(module, binding.attr)
        except AttributeError:
            # `from pkg import submodule`
            return importlib.import_module(f"{binding.module}.{binding.attr}")
    if name == binding.module.partition(".")[0] and "." in binding.module:
        # Unaliased `import a.b` binds the top-level package.
        return sys.modules[name]
    return module


def _evaluate(annotation: Any, globalns: dict[str, Any], localns: dict[str, Any]) -> Any:
    def holder() -> None: ...

    holder.__annotations__ = {"value": annotation}
    return get_type_hints(holder, globalns=globalns, localns=localns, include_extras=False)["value"]


def resolve_hints(
    target: Callable[..., Any], localns: dict[str, Any] | None = None
) -> ResolvedHints:
    """Evaluate each parameter annotation of ``target`` on its own.

    ``NameError``/``AttributeError`` leave that parameter in ``unresolved``.
    Any other failure raises :class:`SpecError` naming the parameter.
    """
    unwrapped = inspect.unwrap(target)
    where = f"{getattr(target, '__module__', '?')}.{getattr(target, '__qualname__', '?')}"
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
        tried: set[str] = set()
        while True:
            try:
                hints[name] = _evaluate(annotation, globalns, local)
            except NameError as exc:
                missing: str = exc.name or ""
                binding = (
                    _type_checking_imports(module).get(missing)
                    if module is not None and missing and missing not in tried
                    else None
                )
                if binding is None:
                    unresolved[name] = exc
                    break
                tried.add(missing)
                try:
                    local[missing] = _import_binding(missing, binding)
                except Exception:  # noqa: BLE001 — a failed rescue leaves the parameter unresolved.
                    unresolved[name] = exc
                    break
                continue
            except AttributeError as exc:
                unresolved[name] = exc
            except TypeError as exc:
                error = unknown_arg_keyword_error(exc, where)
                if error is not None:
                    raise error from exc
                msg = f"can't evaluate the annotation of `{name}` on {where}: {exc}"
                raise SpecError(msg) from exc
            except Exception as exc:
                msg = f"can't evaluate the annotation of `{name}` on {where}: {exc}"
                raise SpecError(msg) from exc
            break
    return ResolvedHints(hints=hints, unresolved=unresolved)
