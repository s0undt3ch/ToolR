from __future__ import annotations

import sys
from pathlib import Path
from types import ModuleType
from unittest.mock import _patch
from unittest.mock import patch

from attrs import define
from attrs import field

from toolr._parser import Parser
from toolr._registry import CommandRegistry


@define(slots=True, frozen=True)
class RegistryTestCase:
    test_case_dir: Path
    parser: Parser = field(init=False, repr=False)
    registry: CommandRegistry = field(init=False, repr=False)
    sys_path: list[str] = field(init=False, repr=False)
    sys_modules: dict[str, ModuleType] = field(init=False, repr=False)
    registry_patcher: _patch = field(init=False, repr=False)
    command_group_patcher: _patch = field(init=False, repr=False)

    @parser.default
    def _default_parser(self) -> Parser:
        return Parser(repo_root=self.test_case_dir)

    @registry.default
    def _default_registry(self) -> CommandRegistry:
        return CommandRegistry(parser=self.parser)

    @sys_path.default
    def _default_sys_path(self) -> list[str]:
        return sys.path[:]

    @sys_modules.default
    def _default_sys_modules(self) -> dict[str, ModuleType]:
        return sys.modules.copy()

    @registry_patcher.default
    def _default_registry_patcher(self) -> _patch:
        return patch("toolr.registry", self.registry)

    @command_group_patcher.default
    def _default_command_group_patcher(self) -> _patch:
        return patch("toolr.command_group", self.registry.command_group)

    def __enter__(self):
        self.registry_patcher.start()
        self.command_group_patcher.start()
        sys.path.insert(0, str(self.test_case_dir))
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        self.registry_patcher.stop()
        self.command_group_patcher.stop()
        sys.path[:] = self.sys_path
        sys.modules.clear()
        sys.modules.update(self.sys_modules)
