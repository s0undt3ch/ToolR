"""
Utilities for testing ToolR and supported commands.
"""

from __future__ import annotations

import sys
from pathlib import Path
from types import ModuleType
from typing import Self
from unittest.mock import _patch
from unittest.mock import patch

from attrs import define
from attrs import field

from toolr._parser import Parser
from toolr._registry import CommandGroup
from toolr._registry import CommandRegistry


@define(slots=True, frozen=True)
class CommandsTester:
    """
    Helper class to simplify testing command discovery.
    """

    search_path: Path
    parser: Parser = field(init=False, repr=False)
    registry: CommandRegistry = field(init=False, repr=False)
    sys_path: list[str] = field(init=False, repr=False)
    sys_modules: dict[str, ModuleType] = field(init=False, repr=False)
    command_group_collector: list[CommandGroup] = field(init=False, repr=False, factory=list)
    command_group_patcher: _patch = field(init=False, repr=False)

    @parser.default
    def _default_parser(self) -> Parser:
        return Parser(repo_root=self.search_path)

    @registry.default
    def _default_registry(self) -> CommandRegistry:
        return CommandRegistry(_parser=self.parser)

    @sys_path.default
    def _default_sys_path(self) -> list[str]:
        return sys.path[:]

    @sys_modules.default
    def _default_sys_modules(self) -> dict[str, ModuleType]:
        # Copy sys.modules but exclude our testing thirdparty package
        return {name: sys.modules[name] for name in sys.modules if not name.startswith(("tools.", "thirdparty"))}

    @command_group_patcher.default
    def _default_command_group_patcher(self) -> _patch:
        return patch("toolr._registry.get_command_group_list", return_value=self.command_group_collector)

    def collected_command_groups(self) -> dict[str, CommandGroup]:
        """
        Get the collected command groups.
        """
        return {group.full_name: group for group in self.command_group_collector}

    def __enter__(self) -> Self:
        """
        Enter the context manager.
        """
        self.command_group_patcher.start()
        sys.path.insert(0, str(self.search_path))
        return self

    def __exit__(self, *args: object) -> None:
        """
        Exit the context manager.
        """
        self.command_group_patcher.stop()
        self.command_group_collector.clear()
        sys.path[:] = self.sys_path
        sys.modules.clear()
        sys.modules.update(self.sys_modules)
