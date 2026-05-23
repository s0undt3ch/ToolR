"""User-facing decorator surface for declaring toolr command groups and commands.

This module survives the retirement of the Python CLI frontend
(``_parser.py`` / ``_registry.py``). User tool scripts continue to do::

    from toolr import command, command_group

and the decorators record metadata in a process-local registry. The
Rust binary's static parser and the manifest builder consume that
registry; there is no longer an in-process Python argparse driver.
"""

from __future__ import annotations

import logging
import warnings
from collections.abc import Callable
from types import FunctionType
from typing import TYPE_CHECKING
from typing import Any
from typing import cast
from typing import overload

from msgspec import Struct
from msgspec import field

from toolr._exc import ToolrDeprecationWarning
from toolr.utils._docstrings import Docstring

if TYPE_CHECKING:
    from toolr.utils._signature import F

log = logging.getLogger(__name__)


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


MANIFEST_SCHEMA_VERSION: int = 1
"""Current toolr manifest fragment schema version.

Mirrors `FRAGMENT_SCHEMA_VERSION` on the Rust side. Bump in lockstep
when introducing a breaking change to the fragment format.
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
    def command(self, name: str) -> Callable[[F], F]: ...

    def command(self, name: str | F) -> Callable[[F], F] | F:
        """Register a new command via the captured group binding.

        The canonical single-file form. Use the standalone
        :func:`toolr.command` decorator with ``group="..."`` when the
        command lives in a different file from its group declaration
        — see *Scaling command groups across files* in the docs.

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
            assert isinstance(name, str)

        def register(func: F) -> F:
            if name in self.__commands:
                log.debug("Command '%s' already exists in group '%s', overriding", name, self.full_name)
            self.__commands[name] = func
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


def command(
    name_or_func: str | Callable[..., Any] | None = None,
    *,
    group: str | None = None,
    aliases: list[str] | None = None,
) -> Callable[..., Any]:
    """Register a function as a toolr CLI command.

    String-path attachment to a group, used as an alternative to the
    legacy ``@<binding>.command`` decorator. Lets you declare commands
    in any file without importing a shared ``CommandGroup`` binding —
    the ``group=`` string is the only contract.

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
        name_or_func: When called as a bare decorator (``@command``),
            this is the wrapped function. When called with parentheses
            (``@command("name", group=...)``), this is the override
            for the command's CLI name; the function name (hyphenated)
            is used otherwise.
        group: Dotted full path of the target group
            (e.g. ``"ci"`` or ``"ci.helm-diff-pr-comment"``). A
            matching ``command_group(...)`` declaration must exist
            elsewhere in ``tools/``; otherwise manifest-build fails
            with a clear error.
        aliases: Reserved for future use; currently no-op (tracked
            with the rest of the ``arg(aliases=...)`` plumbing in
            issue #198).

    Returns:
        The decorated function unchanged; the decorator's only job is
        to record metadata that the static parser picks up. At runtime
        toolr calls the function directly with the parsed args.
    """
    # Bare form: @command def f(...) — no kwargs allowed in this shape.
    if callable(name_or_func):
        if group is not None or aliases is not None:
            err_msg = (
                '@command(...) with kwargs must be used as `@command(group="…")`'
                " — drop the parens-less form or move the kwargs into them."
            )
            raise TypeError(err_msg)
        return name_or_func

    # Parameterised form: @command(...) — returns a decorator. The
    # decorator itself is currently a passthrough; the static parser
    # is what consumes the `group=` / `name` strings.
    def decorator(func: Callable[..., Any]) -> Callable[..., Any]:
        return func

    return decorator


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

    If you pass ``docstring``, you won't be allowed to pass ``description`` or ``long_description``.
    Those will be parsed by [docstring-parser](https://pypi.org/project/docstring-parser/).
    The first line of the docstring will be used as the description, the rest will be used as the long description.

    Args:
        name: Name of the command group; may include dotted parent path.
        title: Optional short title shown in --help. Defaults to the
            leaf name when omitted.
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
    if not title:
        title = name

    collector = _get_command_group_storage()

    group: CommandGroup | None = collector.get(f"{parent}.{name}")
    if group is not None:
        # In this case, we return the existing group
        log.debug("Command group '%s' already exists, returning existing group", f"{parent}.{name}")
        return group

    if docstring is not None:
        if description is not None or long_description is not None:
            err_msg = "You can't pass both docstring and description or long_description"
            raise ValueError(err_msg)
        parsed_docstring = Docstring.parse(docstring)
        description = parsed_docstring.short_description
        long_description = parsed_docstring.long_description
    elif description is None:
        err_msg = "You must at least pass either the 'docstring' or 'description' argument"
        raise ValueError(err_msg)

    if TYPE_CHECKING:
        assert description is not None

    # Create the command group
    collector[f"{parent}.{name}"] = group = CommandGroup(
        name=name,
        title=title,
        description=description,
        parent=parent,
        long_description=long_description,
    )
    return group


__all__ = [
    "MANIFEST_SCHEMA_VERSION",
    "CommandGroup",
    "command",
    "command_group",
]
