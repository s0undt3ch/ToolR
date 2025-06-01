from __future__ import annotations

import importlib
import logging
import pkgutil
from argparse import _SubParsersAction
from collections.abc import Callable
from dataclasses import dataclass
from dataclasses import field
from operator import itemgetter
from pathlib import Path
from typing import Any
from typing import TypeVar

from toolr._parser import Parser

log = logging.getLogger(__name__)

F = TypeVar("F", bound=Callable[..., Any])


@dataclass(frozen=True, slots=True)
class CommandGroup:
    """A group of commands under a common namespace."""

    name: str
    title: str
    description: str
    registry: CommandRegistry
    parent: str | None = None
    _subparsers: _SubParsersAction | None = field(default=None, init=False, repr=False)

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

    def command_group(self, name: str, title: str, description: str) -> CommandGroup:
        """Create a nested command group within this group.

        This is a wrapper around the registry's command_group method that
        passes this group's full name as the parent.

        Args:
            name: Name of the command group
            title: Title for the command group
            description: Description for the command group

        Returns:
            A CommandGroup instance
        """
        return self.registry.command_group(name, title, description, parent=self.full_name)


@dataclass(slots=True)
class CommandRegistry:
    """Registry for CLI commands and their subcommands."""

    parser: Parser = field(default_factory=Parser)
    _command_groups: dict[str, CommandGroup] = field(default_factory=dict, init=False, repr=False)
    _pending_commands: list[dict[str, Any]] = field(default_factory=list, init=False, repr=False)
    _built: bool = field(default=False, init=False, repr=False)

    def _discover_commands(self) -> None:
        """Recursively discover and import command modules from tools/."""
        tools_dir = self.parser.repo_root / "tools"
        if not tools_dir.is_dir():
            return

        def import_commands(path: Path, package: str) -> None:
            log.debug("Importing commands from %s with package %s", path, package)
            for item in pkgutil.iter_modules([str(path)]):
                if item.ispkg:
                    # Recurse into subpackages
                    import_commands(path / item.name, f"{package}.{item.name}")
                else:
                    # Import the module which will trigger command registration
                    importlib.import_module(f"{package}.{item.name}")

        import_commands(tools_dir, "tools")

    def command_group(self, name: str, title: str, description: str, parent: str | None = None) -> CommandGroup:
        """Register a new command group.

        Args:
            name: Name of the command group
            title: Title for the command group
            description: Description for the command group
            parent: Optional parent command path using dot notation (e.g. "tools.docker.build")

        Returns:
            A CommandGroup instance

        """
        if parent is None:
            parent = "tools"
        # Create the command group
        group = CommandGroup(name=name, title=title, description=description, registry=self, parent=parent)

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
            group_parser = parent_subparsers.add_parser(
                group.name, help=f"{group.title} - {group.description}", description=group.description
            )

            # Create subparsers for this group's commands
            subparsers = group_parser.add_subparsers(
                title=group.title, description=group.description, dest=f"{full_name.replace('.', '_')}_command"
            )

            parser_hierarchy[full_name] = subparsers

            # Store reference in the group object
            object.__setattr__(group, "_subparsers", subparsers)

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
            cmd_parser = subparsers.add_parser(cmd_info["name"], help=cmd_info["help"], **cmd_info["kwargs"])
            cmd_parser.set_defaults(func=cmd_info["func"])

        self._built = True

    def discover_and_build(self) -> None:
        """Discover all commands and build the parser hierarchy."""
        self._discover_commands()
        self._build_parsers()


# Global registry instance
registry = CommandRegistry()
