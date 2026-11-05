"""User-facing decorator surface for declaring toolr command groups and commands.

User tool scripts import from this module::

    from toolr import command, command_group

The decorators record metadata in a process-local registry. The Rust
binary's static parser and the manifest builder consume that registry;
the CLI surface itself is a Rust binary, not Python.
"""

from __future__ import annotations

import logging
import sys
import warnings
from collections.abc import Callable
from types import FunctionType
from typing import TYPE_CHECKING
from typing import Any
from typing import NamedTuple
from typing import cast
from typing import overload

from msgspec import Struct
from msgspec import field

from toolr._exc import ToolrDeprecationWarning
from toolr.utils._docstrings import Docstring

if TYPE_CHECKING:
    from toolr.utils._signature import F

log = logging.getLogger(__name__)


def _reject_stripped_docstrings() -> None:
    """Refuse to run where Python has stripped every docstring.

    ``python -OO`` and ``PYTHONOPTIMIZE=2`` both set ``sys.flags.optimize``
    to 2, which leaves every ``__doc__`` as ``None``. toolr can't tell a
    documented command from an undocumented one there.
    """
    if sys.flags.optimize >= 2:
        err_msg = (
            "toolr can't run under `python -OO` or `PYTHONOPTIMIZE=2`: they strip "
            "the docstrings toolr commands and groups are described by."
        )
        raise RuntimeError(err_msg)


def _require_docstring(func: Callable[..., Any]) -> None:
    """Reject a command whose docstring gives no ``--help`` summary line.

    Mirrors the manifest build, so a ``CommandsTester`` discovery test
    fails the same way ``toolr`` would.
    """
    _reject_stripped_docstrings()
    if Docstring.parse(func.__doc__ or "").short_description.strip():
        return
    err_msg = (
        f"{func.__module__}::{func.__qualname__}: add a docstring. "
        "Its first line is the command's `--help` summary."
    )
    raise ValueError(err_msg)


def _emit_legacy_command_group_method_warning(parent_full_name: str, child: str) -> None:
    """Surface a deprecation warning for ``parent.command_group(...)`` usage."""
    parent_leaf = parent_full_name.removeprefix("tools.")
    dotted = f"{parent_leaf}.{child}"
    warnings.warn(
        "CommandGroup.command_group(...) is deprecated and will be removed in toolr 1.0. "
        f"Migrate to `command_group({dotted!r}, ...)`:\n"
        "  from toolr import command_group\n"
        f"  command_group({dotted!r}, ...)\n"
        "Or pass `parent=...` explicitly. "
        "See https://toolr.readthedocs.io/latest/migration/ for the full guide.",
        ToolrDeprecationWarning,
        stacklevel=3,
    )


MANIFEST_SCHEMA_VERSION: int = 2
"""Current toolr manifest schema version.

Mirrors `SCHEMA_VERSION` in `crates/toolr-core/src/manifest/model.rs`.
"""


class CommandGroup(Struct, frozen=True):
    """A group of commands under a common namespace."""

    name: str
    title: str
    description: str
    long_description: str | None = None
    parent: str | None = None
    __commands: dict[str, Callable[..., Any]] = field(default_factory=dict)

    @property
    def full_name(self) -> str:
        """Get the full dot-notation name of this command group."""
        if self.parent is None:
            return self.name
        return f"{self.parent}.{self.name}"

    @overload
    def command(self, name: F) -> F: ...

    @overload
    def command(self, name: str | None = ...) -> Callable[[F], F]: ...

    def command(self, name: str | F | None = None) -> Callable[[F], F] | F:
        """Register a new command via the captured group binding.

        The canonical single-file form. Use the standalone
        :func:`toolr.command` decorator with ``group="..."`` when the
        command lives in a different file from its group declaration
        — see *Scaling command groups across files* in the docs.
        Importing the group binding into another file and using
        ``@group.command`` there also works correctly; it's just less
        locally readable than the standalone ``group=`` form for that
        case.

        Mirrors the standalone :func:`toolr.command` signature (minus
        ``group=``, which the binding already determines). The command
        name may be passed positionally (``@group.command("collect")``)
        or by keyword (``@group.command(name="collect")``) — but not
        both; doing so raises ``TypeError``.

        Args:
            name: Name of the command. If not passed, the function
                name will be used (with underscores converted to
                hyphens).

        Returns:
            A decorator function that registers the command.
        """
        if isinstance(name, FunctionType):
            # Bare-decorator form: `name` is actually the wrapped function.
            # Register it under its (hyphenated) function name and return
            # the function itself.
            cli_name = name.__name__.replace("_", "-")
            inner = cast("Callable[[F], F]", self.command(cli_name))
            return inner(cast("F", name))

        if TYPE_CHECKING:
            assert name is None or isinstance(name, str)

        explicit_name = name

        def register(func: F) -> F:
            cli_name = (
                explicit_name if explicit_name is not None else func.__name__.replace("_", "-")
            )
            _require_docstring(func)
            if cli_name in self.__commands:
                existing = self.__commands[cli_name]
                if existing is not func:
                    _get_duplicate_command_storage().append(
                        DuplicateCommand(self.full_name, cli_name, existing, func)
                    )
                log.debug(
                    "Command '%s' already exists in group '%s', overriding",
                    cli_name,
                    self.full_name,
                )
            self.__commands[cli_name] = func
            return func

        return register

    def command_group(
        self,
        name: str,
        title: str = "",
        description: str | None = None,
        long_description: str | None = None,
        docstring: str | None = None,
    ) -> CommandGroup:
        """Create a nested command group within this group (deprecated method form).

        .. deprecated:: 0.x
            Use the top-level :func:`toolr.command_group` with a dotted
            path (``command_group("parent.child", ...)``) or the
            ``parent="parent"`` keyword instead. Removed in toolr 1.0.

        Returns:
            A CommandGroup instance
        """
        _emit_legacy_command_group_method_warning(self.full_name, name)
        return command_group(
            name,
            title,
            description=description,
            parent=self.full_name,
            long_description=long_description,
            docstring=docstring,
        )

    def get_commands(self) -> dict[str, Callable[..., Any]]:
        """Get the commands in this group."""
        return {name: self.__commands[name] for name in sorted(self.__commands)}


def _get_command_group_storage() -> dict[str, CommandGroup]:
    """Get the list of collected command groups.

    This function acts as a singleton for the list of collected command groups.

    Returns:
        A dictionary of CommandGroup instances by their full name
    """
    try:
        collector = _get_command_group_storage.__command_groups__  # type: ignore[attr-defined]
    except AttributeError:
        command_groups: dict[str, CommandGroup] = {}
        collector = _get_command_group_storage.__command_groups__ = command_groups  # type: ignore[attr-defined]
    return collector


def _group_full_name(group: str) -> str:
    """The registry key for a ``group=`` path: ``"ci.helm"`` is ``"tools.ci.helm"``."""
    return f"tools.{group}"


def _group_path(full_name: str) -> str:
    """The ``group=`` path for a registry key, the inverse of :func:`_group_full_name`."""
    return full_name.removeprefix("tools.")


class PendingCommand(NamedTuple):
    """A standalone ``@command`` waiting for its group to be declared."""

    group: str
    """The ``group=`` path as written; ``""`` when it was missing."""
    name: str
    """The command's CLI name."""
    func: Callable[..., Any]
    """The decorated function."""


def _get_pending_command_storage() -> list[PendingCommand]:
    """Get the standalone commands whose group isn't declared yet.

    Entries are in declaration order. ``command_group()`` takes
    its own entries when it creates the group, so the import order of
    ``tools/`` modules doesn't matter.

    Only ``CommandsTester`` keeps entries: it patches this function to
    return its own list. Nothing else reads them, so by default each call
    returns a new list and the entry is dropped, instead of sitting in
    process-wide state for a later ``command_group()`` to pick up.

    Returns:
        The pending entries
    """
    return []


class DuplicateCommand(NamedTuple):
    """A second function registered under a group's existing command name."""

    group: str
    """The group's full name (``tools.``-prefixed)."""
    name: str
    """The command's CLI name."""
    first: Callable[..., Any]
    """The function already registered under ``name``."""
    second: Callable[..., Any]
    """The function that replaces it."""


def _get_duplicate_command_storage() -> list[DuplicateCommand]:
    """Get the duplicate registrations seen so far.

    Only ``CommandsTester`` keeps entries, by patching this function as it
    does :func:`_get_pending_command_storage`. Elsewhere each call returns a
    new list, so the entry is dropped and the last definition wins; ``toolr``
    refuses such a tree before anything runs.

    Returns:
        The duplicate entries
    """
    return []


@overload
def command(name: F) -> F: ...


@overload
def command(name: str | None = ..., *, group: str | None = ...) -> Callable[[F], F]: ...


def command(
    name: str | Callable[..., Any] | None = None,
    *,
    group: str | None = None,
) -> Callable[..., Any]:
    """Register a function as a toolr CLI command.

    String-path attachment to a group, an alternative to the bound
    ``@<binding>.command`` form. Lets you declare commands in any
    file without importing a shared ``CommandGroup`` binding — the
    ``group=`` string is the only contract.

    The signature mirrors the bound :meth:`CommandGroup.command`
    (plus ``group=``). The command name may be passed positionally
    (``@command("snippet-checker", group=...)``) or by keyword
    (``@command(name="snippet-checker", group=...)``) — but not both;
    doing so raises ``TypeError``.

    toolr reads this decorator from the source without running it. It
    accepts ``@command``, ``@toolr.command`` (also after
    ``import toolr as t``) and an alias from
    ``from toolr import command as cmd``, on a top-level ``def`` with
    ``group=`` and any name as string literals. A nested ``def`` (inside
    another function, a class or an ``if``), ``group=SOME_CONSTANT`` or a
    non-literal name fails the build with "unsupported command
    declarations", and so does :class:`toolr.testing.CommandsTester`.
    A group's command names must be unique within ``tools/``.

    Usage::

        from toolr import command, command_group

        command_group("ci.helm-diff-pr-comment", docstring=__doc__)

        @command(group="ci.helm-diff-pr-comment")
        def backend(ctx, env): ...

        # Optional explicit command name (otherwise the function
        # name is hyphenated and used).
        @command("snippet-checker", group="ci.helm-diff-pr-comment")
        def check_snippets(ctx): ...

    Args:
        name: When called as a bare decorator (``@command``), this is
            the wrapped function. When called with parentheses
            (``@command("name", group=...)`` or
            ``@command(name="name", group=...)``), this is the override
            for the command's CLI name; the function name (hyphenated)
            is used otherwise.
        group: Dotted full path of the target group
            (e.g. ``"ci"`` or ``"ci.helm-diff-pr-comment"``). A
            matching ``command_group(...)`` declaration must exist
            elsewhere in ``tools/``; otherwise manifest-build fails
            with a clear error.

    Returns:
        The decorated function unchanged. With ``group=``, it is also
        recorded under that group, as ``@<binding>.command`` would, so
        :class:`toolr.testing.CommandsTester` sees it. At runtime toolr
        calls the function directly with the parsed args.
    """
    # Bare form: @command def f(...) — no kwargs allowed in this shape.
    if callable(name):
        if group is not None:
            err_msg = (
                '@command(...) with kwargs must be used as `@command(group="…")`'
                " — drop the parens-less form or move the kwargs into them."
            )
            raise TypeError(err_msg)
        _require_docstring(name)
        _record_standalone_command(name, None, None)
        return name

    # Parameterised form: @command(...) — returns a decorator. The static
    # parser reads the `group=` / `name` strings; the registry records them
    # for `CommandsTester`.
    explicit_name = name

    def decorator(func: Callable[..., Any]) -> Callable[..., Any]:
        _require_docstring(func)
        _record_standalone_command(func, explicit_name, group)
        return func

    return decorator


def _record_standalone_command(
    func: Callable[..., Any], explicit_name: str | None, group: str | None
) -> None:
    """Attach a standalone ``@command`` to its group, or park it as pending.

    A command with no ``group=`` (or ``group=""``) is parked under ``""``,
    which no group ever takes, so ``CommandsTester`` reports it as the
    manifest build does.
    """
    cli_name = explicit_name if explicit_name is not None else func.__name__.replace("_", "-")
    existing = _get_command_group_storage().get(_group_full_name(group)) if group else None
    if existing is not None:
        existing.command(cli_name)(func)
    else:
        # The group's module may be imported later; `command_group()` attaches it then.
        _get_pending_command_storage().append(PendingCommand(group or "", cli_name, func))


def _resolve_group_fields(
    title: str,
    description: str | None,
    long_description: str | None,
    docstring: str | None,
    *,
    exists: bool = False,
) -> tuple[str, str, str | None]:
    """Validate ``command_group`` arguments and resolve the final group fields.

    A description is only required to create a group. For an ``exists`` group a
    missing one is a re-fetch, so the returned description is empty.

    Returns:
        The ``(title, description, long_description)`` the group is created with
    """
    if docstring is None and description is None:
        _reject_stripped_docstrings()
    if docstring is not None:
        if description is not None or long_description is not None:
            err_msg = "You can't pass both docstring and description or long_description"
            raise ValueError(err_msg)
        # Mirror the static parser (parser/groups.rs) and the `@command`
        # path (parser/commands.rs): the docstring's first paragraph
        # populates `title` (clap's `about`); the Rust-rendered
        # ``full_description`` (short + long + Examples/Notes/…)
        # populates `description` (clap's `long_about`). An explicit
        # `title=` always wins for the short slot.
        parsed_docstring = Docstring.parse(docstring)
        if not title:
            title = parsed_docstring.short_description
        return title, parsed_docstring.full_description, None
    if description is None and not exists:
        err_msg = "You must at least pass either the 'docstring' or 'description' argument"
        raise ValueError(err_msg)
    return title, description or "", long_description


def command_group(
    name: str,
    title: str = "",
    description: str | None = None,
    long_description: str | None = None,
    docstring: str | None = None,
    parent: str | None = None,
) -> CommandGroup:
    """Register a new command group.

    The ``name`` may be either a bare leaf name (``"ci"``) or a dotted
    path (``"ci.helm-diff-pr-comment"``). When dotted, everything
    before the final dot is the parent's full path; explicit
    ``parent=`` is ignored in that case.

    If you pass ``docstring``, you won't be allowed to pass ``description`` or
    ``long_description``. The docstring is parsed with the same parser used for
    ``@command`` function docstrings: the first paragraph becomes the group's
    ``title`` (clap's ``about``, shown next to the group name in the parent's
    command listing) and the remainder becomes the ``description`` (clap's
    ``long_about``, shown when the user runs ``toolr <group> --help``). Pass an
    explicit ``title=`` or ``description=`` to override either side.

    Args:
        name: Name of the command group; may include dotted parent path.
        title: Optional short title shown next to the group name in the
            parent ``--help`` listing. Defaults to the docstring's first
            paragraph when ``docstring=`` is supplied, otherwise empty.
        description: Description for the command group
        long_description: Long description for the command group
        docstring: Docstring for the command group
        parent: Optional parent command path using dot notation (e.g. "tools.docker.build")

    Returns:
        A CommandGroup instance

    """
    # Dotted-path form: split the leaf off the parent path. Explicit
    # `parent=` kwarg takes a back seat to the dotted form so users
    # pick one style and stick with it.
    if "." in name:
        parent_from_path, _, leaf = name.rpartition(".")
        if parent is not None and parent != parent_from_path:
            log.warning(
                "command_group(%r, parent=%r): explicit parent overridden by dotted path; using %r",
                name,
                parent,
                parent_from_path,
            )
        name = leaf
        parent = parent_from_path
    if parent is not None and not parent.startswith("tools."):
        parent = f"tools.{parent}"
    elif parent is None:
        parent = "tools"

    collector = _get_command_group_storage()
    group: CommandGroup | None = collector.get(f"{parent}.{name}")

    # Contradictory arguments raise whether or not the group exists; a missing
    # description only raises when this call would have to create the group.
    title, description, long_description = _resolve_group_fields(
        title, description, long_description, docstring, exists=group is not None
    )

    if group is not None:
        # The first declaration wins; flag a later one that disagrees with it
        # on a field it actually provided.
        differing = [
            field
            for field, wanted in (
                ("title", title),
                ("description", description),
                ("long_description", long_description),
            )
            if wanted and getattr(group, field) != wanted
        ]
        if differing:
            log.debug(
                "Command group '%s' is already declared; ignoring the different %s of this declaration",
                group.full_name,
                ", ".join(differing),
            )
        else:
            log.debug(
                "Command group '%s' already exists, returning existing group", group.full_name
            )
        return group

    # Create the command group
    collector[f"{parent}.{name}"] = group = CommandGroup(
        name=name,
        title=title,
        description=description,
        parent=parent,
        long_description=long_description,
    )
    pending = _get_pending_command_storage()
    waiting = [entry for entry in pending if _group_full_name(entry.group) == group.full_name]
    pending[:] = [entry for entry in pending if _group_full_name(entry.group) != group.full_name]
    for entry in waiting:
        group.command(entry.name)(entry.func)
    return group


__all__ = [
    "MANIFEST_SCHEMA_VERSION",
    "CommandGroup",
    "command",
    "command_group",
]
