from __future__ import annotations

import importlib
import logging
import os
import pkgutil
import warnings
from argparse import _SubParsersAction
from collections.abc import Callable
from operator import attrgetter
from pathlib import Path
from types import FunctionType
from typing import TYPE_CHECKING
from typing import Any
from typing import cast
from typing import overload

from msgspec import Struct
from msgspec import field
from msgspec import structs
from rich.markdown import Markdown

from toolr._exc import ToolrDeprecationWarning
from toolr.utils._docstrings import Docstring
from toolr.utils._signature import F
from toolr.utils._signature import get_signature

if TYPE_CHECKING:
    from toolr._parser import Parser

log = logging.getLogger(__name__)


def _emit_legacy_command_warning(group_full_name: str) -> None:
    """Surface a deprecation warning for ``@<binding>.command`` usage.

    Fires on every legacy decorator invocation; the runner installs a
    ``"default"`` filter so each call site warns once per process,
    keeping output noise bounded.
    """
    leaf = group_full_name.removeprefix("tools.")
    warnings.warn(
        "@<binding>.command is deprecated and will be removed in toolr 1.0. "
        f"Migrate to `@command(group={leaf!r})`:\n"
        "  from toolr import command, command_group\n"
        f"  command_group({leaf!r}, ...)\n"
        f"  @command(group={leaf!r})\n"
        "  def my_command(ctx, ...): ...\n"
        "See https://s0undt3ch.github.io/ToolR/migration/ for the full guide.",
        ToolrDeprecationWarning,
        stacklevel=3,
    )


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
        "See https://s0undt3ch.github.io/ToolR/migration/ for the full guide.",
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
        """Register a new command (deprecated binding-style decorator).

        .. deprecated:: 0.x
            Use :func:`toolr.command` with ``group="..."`` instead.
            Removed in toolr 1.0.

        Args:
            name: Name of the command. If not passed, the function name will be used.

        Returns:
            A decorator function that registers the command
        """
        _emit_legacy_command_warning(self.full_name)
        return self._command(name)

    def _command(self, name: str | F) -> Callable[[F], F] | F:
        """Internal helper used by :meth:`command` after deprecation accounting."""
        if isinstance(name, FunctionType):
            # Bare-decorator form: `name` is actually the wrapped function.
            # Register it under its (hyphenated) function name and return
            # the function itself.
            cli_name = name.__name__.replace("_", "-")
            inner = cast("Callable[[F], F]", self._command(cli_name))
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


class CommandRegistry(Struct, frozen=True):
    """Registry for CLI commands and their subcommands."""

    _built: bool = False
    _parser: Parser | None = None

    @property
    def parser(self) -> Parser:
        """Get the parser for this registry."""
        if self._parser is None:
            err_msg = "The parser is not set. Please pass a parser instance when calling self.discover_and_build()"
            raise RuntimeError(err_msg)
        return self._parser

    def _set_parser(self, parser: Parser) -> None:
        """Set the parser for this registry."""
        if self._parser is not None:
            err_msg = "A parser has already been set?!"
            raise RuntimeError(err_msg)
        structs.force_setattr(self, "_parser", parser)

    def _discover_commands(self) -> None:
        """Discover both project local as well as 3rd party commands."""
        self._discover_entry_points_commands()
        self._discover_local_commands()

    def _discover_local_commands(self) -> None:
        """Recursively discover and import command modules from tools/."""
        tools_dir = self.parser.repo_root / "tools"
        if not tools_dir.is_dir():
            return

        def import_commands(path: Path, package: str) -> None:
            log.debug("Importing commands from %s with package %s", path, package)
            for item in pkgutil.iter_modules([str(path)]):
                try:
                    if item.ispkg:
                        # Recurse into subpackages
                        import_commands(path / item.name, f"{package}.{item.name}")
                    else:
                        # Import the module which will trigger command registration
                        importlib.import_module(f"{package}.{item.name}")
                except ModuleNotFoundError as exc:
                    # If we're not debugging imports, we don't want to raise an error
                    if os.environ.get("TOOLR_DEBUG_IMPORTS", "0") == "1":
                        raise exc from None
                except ImportError as exc:
                    # This is likely something wrong with the environment, raise it to the user
                    raise exc from None

        import_commands(tools_dir, "tools")

    def _discover_entry_points_commands(self) -> None:
        """Discover and import command modules from entry points."""
        for entry_point in importlib.metadata.entry_points(group="toolr.tools"):
            log.debug("Importing commands from entry point %s", entry_point.module)
            entry_point.load()

    def _build_parsers(self) -> None:
        """Build the argument parsers from the registered command groups and commands."""
        if self._built:
            return

        # Create a hierarchy of subparsers based on the dot notation paths
        parser_hierarchy: dict[str, _SubParsersAction] = {}

        collector = _get_command_group_storage()

        for group in sorted(collector.values(), key=attrgetter("full_name")):
            if group.parent == "tools":
                # Top-level command group
                parent_subparsers = self.parser.subparsers
            else:
                # Nested command group
                if group.parent not in parser_hierarchy:
                    # Parent doesn't exist yet - this shouldn't happen with our sorting
                    err_msg = (
                        f"Parent command group '{group.parent}' for command '{group.name}' "
                        "does not exist. Please check your code."
                    )
                    raise ValueError(err_msg)
                parent_subparsers = parser_hierarchy[group.parent]

            # Create subparsers for this group
            if TYPE_CHECKING:
                assert parent_subparsers is not None

            # Cast description to str to satisfy mypy since the formatter_class will know what to do with it
            group_parser_description = cast("str", Markdown(group.description, style="argparse.text", justify="left"))
            group_parser = parent_subparsers.add_parser(
                group.name,
                help=f"{group.title} - {group.description}",
                description=group_parser_description,
                formatter_class=self.parser.formatter_class,
            )

            # Create subparsers for this group's commands
            subparsers_description = cast(
                "str", Markdown(group.long_description or group.description, style="argparse.text", justify="left")
            )
            group_full_name = group.full_name
            subparsers = group_parser.add_subparsers(
                title=group.title,
                description=subparsers_description,
                dest=f"{group_full_name.replace('.', '_')}_command",
            )

            parser_hierarchy[group_full_name] = subparsers

            # Now add all the pending commands to their respective groups
            commands = group.get_commands()
            for command_name in commands:
                signature = get_signature(commands[command_name])
                long_command_description = cast(
                    "str", Markdown(signature.long_description, style="argparse.text", justify="left")
                )
                cmd_parser = subparsers.add_parser(
                    command_name,
                    help=signature.short_description,
                    description=long_command_description,
                    formatter_class=self.parser.formatter_class,
                )
                signature.setup_parser(cmd_parser)

        structs.force_setattr(self, "_built", True)  # noqa: FBT003

    def discover_and_build(self, parser: Parser | None = None) -> None:
        """Discover all commands and build the parser hierarchy."""
        if parser is not None:
            self._set_parser(parser)
        self._discover_commands()
        self._build_parsers()


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
