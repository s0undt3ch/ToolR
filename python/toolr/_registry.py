from __future__ import annotations

import importlib
import logging
import os
import pkgutil
from argparse import _SubParsersAction
from collections.abc import Callable
from operator import itemgetter
from pathlib import Path
from typing import TYPE_CHECKING
from typing import Any
from typing import TypeVar

from msgspec import Struct
from msgspec import field
from msgspec import structs
from rich.markdown import Markdown

if TYPE_CHECKING:
    from toolr._parser import Parser

log = logging.getLogger(__name__)

F = TypeVar("F", bound=Callable[..., Any])


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

    def command(self, name: str, help: str = "", **kwargs: Any) -> Callable[[F], F]:  # noqa: A002
        """Register a new command.

        Args:
            name: Name of the command
            help: Help text for the command
            **kwargs: Additional arguments passed to add_parser()

        Returns:
            A decorator function that registers the command
        """

        def decorator(func: F) -> F:
            self.registry._pending_commands.append(  # noqa: SLF001
                {"group_path": self.full_name, "name": name, "help": help, "func": func, "kwargs": kwargs}
            )
            return func

        return decorator

    def command_group(
        self, name: str, title: str, description: str, long_description: str | None = None
    ) -> CommandGroup:
        """Create a nested command group within this group.

        This is a wrapper around the registry's command_group method that
        passes this group's full name as the parent.

        Args:
            name: Name of the command group
            title: Title for the command group
            description: Description for the command group
            long_description: Long description for the command group

        Returns:
            A CommandGroup instance
        """
        return self.registry.command_group(
            name,
            title,
            description,
            parent=self.full_name,
            long_description=long_description,
        )


class CommandRegistry(Struct, frozen=True):
    """Registry for CLI commands and their subcommands."""

    _command_groups: dict[str, CommandGroup] = field(default_factory=dict)
    _pending_commands: list[dict[str, Any]] = field(default_factory=list)
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
        description: str,
        long_description: str | None = None,
        parent: str | None = None,
    ) -> CommandGroup:
        """Register a new command group.

        Args:
            name: Name of the command group
            title: Title for the command group
            description: Description for the command group
            long_description: Long description for the command group
            parent: Optional parent command path using dot notation (e.g. "tools.docker.build")

        Returns:
            A CommandGroup instance

        """
        if parent is None:
            parent = "tools"
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

            group_parser = parent_subparsers.add_parser(
                group.name,
                help=f"{group.title} - {group.description}",
                description=Markdown(group.description, style="argparse.text"),
                formatter_class=self.parser.formatter_class,
            )

            # Create subparsers for this group's commands
            subparsers = group_parser.add_subparsers(
                title=group.title,
                description=Markdown(group.long_description or group.description, style="argparse.text"),
                dest=f"{full_name.replace('.', '_')}_command",
            )

            parser_hierarchy[full_name] = subparsers

            # Store reference in the group object
            structs.force_setattr(group, "_subparsers", subparsers)

        # Now add all the pending commands to their respective groups
        for cmd_info in sorted(self._pending_commands, key=itemgetter("group_path")):
            group_path = cmd_info["group_path"]
            if group_path not in parser_hierarchy:
                # This shouldn't happen with our sorting and because we also check
                # for this when building the subparsers.
                err_msg = (
                    f"Command group '{group_path}' for command '{cmd_info['name']}' "
                    "does not exist. Please check your code."
                )
                raise ValueError(err_msg)

            subparsers = parser_hierarchy[group_path]
            cmd_parser = subparsers.add_parser(
                cmd_info["name"],
                help=cmd_info["help"],
                description=cmd_info["kwargs"].get("description", ""),
                **cmd_info["kwargs"],
                formatter_class=self.parser.formatter_class,
            )
            cmd_parser.set_defaults(func=cmd_info["func"])

        structs.force_setattr(self, "_built", True)  # noqa: FBT003

    def discover_and_build(self, parser: Parser | None = None) -> None:
        """Discover all commands and build the parser hierarchy."""
        if parser is not None:
            self._set_parser(parser)
        self._discover_commands()
        self._build_parsers()


# Global registry instance
registry = CommandRegistry()
