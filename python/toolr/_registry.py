from __future__ import annotations

import importlib
import logging
import os
import pkgutil
from argparse import _SubParsersAction
from collections.abc import Callable
from operator import itemgetter
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

from toolr.utils._docstrings import parse_docstring
from toolr.utils._signature import F
from toolr.utils._signature import get_signature

if TYPE_CHECKING:
    from toolr._parser import Parser

log = logging.getLogger(__name__)


class CommandGroup(Struct, frozen=True):
    """A group of commands under a common namespace."""

    name: str
    title: str
    description: str
    registry: CommandRegistry
    long_description: str | None = None
    parent: str | None = None
    _subparsers: _SubParsersAction | None = None

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
        """Register a new command.

        Args:
            name: Name of the command. If not passed, the function name will be used.

        Returns:
            A decorator function that registers the command
        """
        if isinstance(name, FunctionType):
            # If we were not passed a name in the decorator call, we're being called with a function
            # and we need to use the function name as the command name
            return self.command(name.__name__.replace("_", "-"))(name)

        if TYPE_CHECKING:
            assert isinstance(name, str)

        def decorator(func: F) -> F:
            self.registry._pending_commands.append((self.full_name, name, func))  # noqa: SLF001
            return func

        return decorator

    def command_group(
        self,
        name: str,
        title: str,
        description: str | None = None,
        long_description: str | None = None,
        docstring: str | None = None,
    ) -> CommandGroup:
        """Create a nested command group within this group.

        This is a wrapper around the [CommandRegistry.command_group][toolr._registry.CommandRegistry.command_group]
        method that passes this group's full name as the parent.

        See [command_group][toolr._registry.CommandRegistry.command_group] for more details.

        Returns:
            A CommandGroup instance
        """
        return self.registry.command_group(
            name,
            title,
            description=description,
            parent=self.full_name,
            long_description=long_description,
            docstring=docstring,
        )


class CommandRegistry(Struct, frozen=True):
    """Registry for CLI commands and their subcommands."""

    _command_groups: dict[str, CommandGroup] = field(default_factory=dict)
    _pending_commands: list[tuple[str, str, Callable[..., Any]]] = field(default_factory=list)
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
                except ImportError as exc:
                    if os.environ.get("TOOLR_DEBUG_IMPORTS", "0") == "1":
                        raise exc from None

        import_commands(tools_dir, "tools")

    def command_group(
        self,
        name: str,
        title: str,
        description: str | None = None,
        long_description: str | None = None,
        docstring: str | None = None,
        parent: str | None = None,
    ) -> CommandGroup:
        """Register a new command group.

        If you pass ``docstring``, you won't be allowed to pass ``description`` or ``long_description``.
        Those will be parsed by [docstring-parser](https://pypi.org/project/docstring-parser/).
        The first line of the docstring will be used as the description, the rest will be used as the long description.

        Args:
            name: Name of the command group
            title: Title for the command group
            description: Description for the command group
            long_description: Long description for the command group
            docstring: Docstring for the command group
            parent: Optional parent command path using dot notation (e.g. "tools.docker.build")

        Returns:
            A CommandGroup instance

        """
        if parent is None:
            parent = "tools"

        if docstring is not None:
            if description is not None or long_description is not None:
                err_msg = "You can't pass both docstring and description or long_description"
                raise ValueError(err_msg)
            parsed_docstring = parse_docstring(docstring)
            description = parsed_docstring.short_description
            long_description = parsed_docstring.long_description
        elif description is None:
            err_msg = "You must at least pass either the 'docstring' or 'description' argument"
            raise ValueError(err_msg)

        if TYPE_CHECKING:
            assert description is not None

        # Create the command group
        group = CommandGroup(
            name=name,
            title=title,
            description=description,
            registry=self,
            parent=parent,
            long_description=long_description,
        )

        # Store the command group for later parser building
        self._command_groups[group.full_name] = group
        return group

    def _build_parsers(self) -> None:
        """Build the argument parsers from the registered command groups and commands."""
        if self._built:
            return

        # Create a hierarchy of subparsers based on the dot notation paths
        parser_hierarchy: dict[str, _SubParsersAction] = {}

        for full_name, group in sorted(self._command_groups.items()):
            # Sanity check
            assert group.full_name == full_name  # noqa: S101

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
            group_parser_description = cast("str", Markdown(group.description, style="argparse.text"))
            group_parser = parent_subparsers.add_parser(
                group.name,
                help=f"{group.title} - {group.description}",
                description=group_parser_description,
                formatter_class=self.parser.formatter_class,
            )

            # Create subparsers for this group's commands
            subparsers_description = cast(
                "str", Markdown(group.long_description or group.description, style="argparse.text")
            )
            subparsers = group_parser.add_subparsers(
                title=group.title,
                description=subparsers_description,
                dest=f"{full_name.replace('.', '_')}_command",
            )

            parser_hierarchy[full_name] = subparsers

            # Store reference in the group object
            structs.force_setattr(group, "_subparsers", subparsers)

        # Now add all the pending commands to their respective groups
        for group_path, command_name, func in sorted(self._pending_commands, key=itemgetter(0)):
            if group_path not in parser_hierarchy:
                # This shouldn't happen with our sorting and because we also check
                # for this when building the subparsers.
                err_msg = (
                    f"Command group '{group_path}' for command '{command_name}' does not exist. Please check your code."
                )
                raise ValueError(err_msg)

            signature = get_signature(func)

            subparsers = parser_hierarchy[group_path]
            cmd_parser = subparsers.add_parser(
                command_name,
                help=signature.short_description,
                description=signature.long_description,
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


# Global registry instance
registry = CommandRegistry()
