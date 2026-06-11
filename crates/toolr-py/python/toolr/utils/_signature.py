"""
Utilities to parse function signatures.
"""

from __future__ import annotations

import inspect
import warnings
from collections.abc import Callable
from typing import Any
from typing import Literal
from typing import TypeAlias
from typing import TypeVar
from typing import get_type_hints

from msgspec import Struct

from toolr._exc import ToolrDeprecationWarning

F = TypeVar("F", bound=Callable[..., Any])
NargsType: TypeAlias = Literal["*", "+", "?"] | int


class ArgSection(Struct, frozen=True):
    """A named --help section for grouping related arguments.

    Declare once at module scope, then reference it from each member
    argument's :func:`arg` annotation. The rust front-end renders the
    title as a clap help-heading and (when present) prints the
    description as a one-line prose blurb under it.

    Identity-based: two ``ArgSection(title="...")`` instances with the
    same title are *not* the same section. Define the section once and
    reuse the same Python object so a typo can't silently create a new
    section.

    Example::

        from toolr import arg, arg_section, command

        LOGGING = arg_section("Logging Options",
                              description="Control verbosity and output.")

        @command(group="example")
        def hello(
            ctx,
            verbose: Annotated[bool, arg(help_section=LOGGING)] = False,
            quiet: Annotated[bool, arg(help_section=LOGGING,
                                       conflicts_with=["verbose"])] = False,
        ): ...
    """

    title: str
    description: str | None = None


def arg_section(title: str, *, description: str | None = None) -> ArgSection:
    """Construct an :class:`ArgSection` for use as ``arg(help_section=...)``.

    Args:
        title: Short heading shown in ``--help`` above the section's
            arguments. Renders as a clap help-heading.
        description: Optional one-line prose displayed under the
            heading. Markdown is supported via the same termimad
            renderer toolr uses elsewhere.

    Returns:
        An ``ArgSection`` instance — store it at module scope and pass
        it (by reference, not by re-instantiating) to every member
        argument.
    """
    return ArgSection(title=title, description=description)


class ArgumentAnnotation(Struct, frozen=True):
    """Metadata harvested from ``Annotated[T, arg(...)]``.

    The python runtime keeps only the fields it actually uses at
    invocation time; the rust static parser independently reads the
    same call expression and harvests the kwargs *it* uses, so the
    two sides don't need a serialised representation of this struct.
    """

    # Plumbed through to clap by the rust front-end.
    aliases: list[str] | None = None
    metavar: str | None = None
    env: str | None = None
    hide: bool = False
    help_section: ArgSection | None = None
    display_order: int | None = None
    conflicts_with: list[str] | None = None
    requires: list[str] | None = None
    # Path constraints — only meaningful for Path-typed parameters.
    must_exist: bool = False
    must_be_file: bool = False
    must_be_dir: bool = False
    # Deprecated kwargs — kept on the struct so existing call sites
    # don't TypeError, but every one of them emits a
    # `ToolrDeprecationWarning` from `arg()`.
    required: bool | None = None
    action: str | None = None
    choices: list[Any] | None = None
    nargs: NargsType | None = None
    group: str | None = None


def _deprecated(name: str, *, replacement: str) -> None:
    warnings.warn(
        f"arg({name}=...) is deprecated and will be removed in toolr 1.0. {replacement}",
        ToolrDeprecationWarning,
        stacklevel=3,
    )


# Order-preserving string collection: ``list`` and ``tuple``. Used
# for kwargs where declaration order is semantically meaningful (e.g.
# ``aliases``, whose first short entry becomes clap's ``Arg::short``).
_StrSequence: TypeAlias = list[str] | tuple[str, ...]

# Order-insensitive string collection: ``list`` / ``tuple`` /
# ``set`` / ``frozenset``. Used for kwargs whose values are
# semantically set-like (``conflicts_with``, ``requires``).
_StrCollection: TypeAlias = list[str] | tuple[str, ...] | set[str] | frozenset[str]


def _validate_str_elements(name: str, items: list[str]) -> None:
    for i, item in enumerate(items):
        if not isinstance(item, str):
            msg = f"arg(): `{name}=` element [{i}] must be a string, got {type(item).__name__} ({item!r})."
            raise TypeError(msg)


def _coerce_str_sequence_kwarg(name: str, value: _StrSequence | None) -> list[str] | None:
    """Order-preserving validator for ``aliases``.

    Accepts ``list`` or ``tuple`` — both preserve declaration order,
    which matters because the first short entry becomes clap's
    ``Arg::short``. Sets and frozensets are rejected: they have no
    observable order and using them here is almost always a mistake.

    Bare strings are rejected because ``str`` is itself iterable —
    silently iterating yields one character per element, which is
    never what the caller meant.
    """
    if value is None:
        return None
    if isinstance(value, str):
        msg = (
            f"arg(): `{name}=` must be a list or tuple of strings, got a bare "
            f"`str` ({value!r}). Wrap a single item as `{name}=[{value!r}]`."
        )
        raise TypeError(msg)
    if not isinstance(value, (list, tuple)):
        msg = f"arg(): `{name}=` must be a list or tuple of strings, got {type(value).__name__}."
        raise TypeError(msg)
    items = list(value)
    _validate_str_elements(name, items)
    return items


def _coerce_str_collection_kwarg(name: str, value: _StrCollection | None) -> list[str] | None:
    """Order-insensitive validator for ``conflicts_with`` / ``requires``.

    These fields are semantically set-like: "this flag conflicts
    with these other flags." Accept ``list`` / ``tuple`` / ``set`` /
    ``frozenset`` and materialise to a ``list[str]`` so the rest of
    the pipeline sees a uniform shape. The Rust AST parser mirrors
    this and statically extracts ``[...]`` / ``(...)`` / ``{...}``
    literals; anything more dynamic (name references, function
    calls) is runtime-only and will not appear in the static
    manifest.

    Bare strings are rejected because ``str`` is itself iterable —
    silently iterating yields one character per element, which is
    never what the caller meant.
    """
    if value is None:
        return None
    if isinstance(value, str):
        msg = (
            f"arg(): `{name}=` must be a list, tuple, or set of strings, got a "
            f"bare `str` ({value!r}). Wrap a single item as `{name}=[{value!r}]`."
        )
        raise TypeError(msg)
    if not isinstance(value, (list, tuple, set, frozenset)):
        msg = f"arg(): `{name}=` must be a list, tuple, or set of strings, got {type(value).__name__}."
        raise TypeError(msg)
    items = list(value)
    _validate_str_elements(name, items)
    return items


def arg(  # noqa: PLR0913 — kwargs surface mirrors the clap features we expose; each is intentionally distinct.
    *,
    # Active kwargs (plumbed through to clap).
    aliases: _StrSequence | None = None,
    metavar: str | None = None,
    env: str | None = None,
    hide: bool = False,
    help_section: ArgSection | None = None,
    display_order: int | None = None,
    conflicts_with: _StrCollection | None = None,
    requires: _StrCollection | None = None,
    must_exist: bool = False,
    must_be_file: bool = False,
    must_be_dir: bool = False,
    # Deprecated legacy kwargs. Each emits a `ToolrDeprecationWarning`
    # and (when applicable) maps onto the new field internally.
    required: bool | None = None,
    action: str | None = None,
    choices: list[Any] | None = None,
    nargs: NargsType | None = None,
    group: str | None = None,
) -> ArgumentAnnotation:
    """Create an :class:`ArgumentAnnotation` for use with ``typing.Annotated``.

    Args:
        aliases: Extra short or long flag spellings (e.g. ``["-n", "--who"]``).
            Single-character entries become clap shorts; longer entries
            become aliases.
        metavar: Custom placeholder shown in ``--help``
            (e.g. ``"PATH"`` → ``--config <PATH>``).
        env: Read the default from this environment variable when the
            flag isn't passed. Maps to clap ``Arg::env(...)``.
        hide: When ``True``, omit the argument from ``--help`` output.
            Still parseable on the command line.
        help_section: An :class:`ArgSection` returned by
            :func:`arg_section`. Groups related arguments under a
            named heading in ``--help``.
        display_order: Integer ordering hint for ``--help`` rendering.
            Lower values render first; arguments without a value fall
            back to source order.
        conflicts_with: Names of other parameters that may not be used
            together with this one.
        requires: Names of other parameters that must also be set when
            this one is.
        must_exist: For path-typed params: reject paths that don't
            exist on disk. Useful for "input file" style arguments.
        must_be_file: For path-typed params: also require the path
            is a regular file. Implies ``must_exist=True``.
        must_be_dir: For path-typed params: also require the path
            is a directory. Implies ``must_exist=True``.
        required: **Deprecated.** Removed in 1.0. Use ``T | None`` or
            ``*args: T`` to express optional / zero-or-more.
        action: **Deprecated.** Removed in 1.0. ``bool`` defaults imply
            flag actions; ``list[T]`` implies append; ``Count`` implies
            counting.
        choices: **Deprecated.** Removed in 1.0. Use ``Literal["a","b"]``
            or an :class:`enum.Enum` subclass instead.
        nargs: **Deprecated.** Removed in 1.0. ``T | None``,
            ``*args: T``, and ``tuple[T1, T2]`` cover the cases.
        group: **Deprecated.** Removed in 1.0. Use
            ``conflicts_with=[...]`` for mutex relationships and
            ``help_section=`` for display grouping.
    """
    aliases = _coerce_str_sequence_kwarg("aliases", aliases)
    conflicts_with = _coerce_str_collection_kwarg("conflicts_with", conflicts_with)
    requires = _coerce_str_collection_kwarg("requires", requires)
    if required is not None:
        _deprecated(
            "required",
            replacement=(
                "Use `T | None` (with a default of None) for optional args, or `*args: T` for zero-or-more positionals."
            ),
        )
    if action is not None:
        _deprecated(
            "action",
            replacement=(
                "Action is inferred from the parameter type: `bool` → flag, "
                "`list[T]` → append, `toolr.types.Count` → count."
            ),
        )
    if choices is not None:
        _deprecated(
            "choices",
            replacement=(
                "Use `Literal['a', 'b']` or an `enum.Enum` subclass instead — "
                "the choices come from the type annotation."
            ),
        )
    if nargs is not None:
        _deprecated(
            "nargs",
            replacement=(
                "Use `T | None` (zero-or-one), `*args: T` (zero-or-more), "
                "or `tuple[T1, T2, ...]` (fixed arity) — all driven by the type."
            ),
        )
    if group is not None:
        _deprecated(
            "group",
            replacement=(
                "Use `conflicts_with=[...]` for mutually-exclusive arguments "
                "or `help_section=` (via `arg_section(...)`) for display grouping."
            ),
        )
    return ArgumentAnnotation(
        aliases=aliases,
        metavar=metavar,
        env=env,
        hide=hide,
        help_section=help_section,
        display_order=display_order,
        conflicts_with=conflicts_with,
        requires=requires,
        must_exist=must_exist,
        must_be_file=must_be_file,
        must_be_dir=must_be_dir,
        required=required,
        action=action,
        choices=choices,
        nargs=nargs,
        group=group,
    )


class DispatcherDetectionError(Exception):
    """Raised when a function's DispatchCommand usage is malformed."""


def detect_dispatch_parameter(func: Callable[..., Any]) -> str | None:
    """Return the name of the function's `DispatchCommand` parameter, or None.

    A command qualifies as a dispatcher iff exactly one keyword-only
    parameter is annotated with `toolr.sources.DispatchCommand`. The
    parameter name itself is free. Subclasses are not supported in v1.
    Returns `None` when the function isn't a dispatcher; raises
    `DispatcherDetectionError` on a malformed usage.
    """
    # Local import: keep toolr.sources out of the import-time graph of
    # toolr.utils._signature.
    from toolr.sources import DispatchCommand  # noqa: PLC0415

    sig = inspect.signature(func)
    # Resolve string annotations (PEP 563 / ``from __future__ import annotations``)
    # so the identity check against ``DispatchCommand`` works regardless of
    # how the caller declared the parameter.
    #
    # Forward references that can't be resolved at runtime (NameError /
    # AttributeError) — typically TYPE_CHECKING-only imports — fall back
    # to whatever ``inspect.signature`` recorded. A ``TypeError`` here,
    # however, comes from evaluating ``Annotated[..., metadata(...)]``
    # where the metadata constructor itself rejected its arguments
    # (e.g. ``arg(conflicts_with="foo")`` instead of ``[...]``). That's
    # a real bug in the caller's annotation that masks dispatch detection
    # if swallowed, so we let it propagate with its original message.
    try:
        resolved = get_type_hints(func)
    except (NameError, AttributeError):
        resolved = {}

    found_kw: list[str] = []
    for name, param in sig.parameters.items():
        annotation = resolved.get(name, param.annotation)
        if annotation is inspect.Parameter.empty:
            continue
        if annotation is not DispatchCommand:
            continue
        if param.kind != inspect.Parameter.KEYWORD_ONLY:
            msg = (
                f"DispatchCommand parameter {name!r} on {func.__qualname__!r} must be keyword-only"
            )
            raise DispatcherDetectionError(msg)
        found_kw.append(name)

    if len(found_kw) > 1:
        msg = f"{func.__qualname__!r} declares more than one DispatchCommand parameter: {found_kw}"
        raise DispatcherDetectionError(msg)
    return found_kw[0] if found_kw else None
