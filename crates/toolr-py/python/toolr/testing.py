"""
Utilities for testing ToolR and supported commands.

Survives the retirement of the Python CLI frontend
(``_parser.py`` / ``_registry.py``). Instead of the legacy in-process
``CommandRegistry``, this helper patches the registry storage in
:mod:`toolr._decorators` and drives the same Python-side discovery
that the Rust binary's dynamic manifest layer uses
(``toolr._introspect.build_payload``).

The API surface remains the same: ``with CommandsTester(search_path=...) as t``
yields an isolated collector available via ``t.collected_command_groups()``.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path
from types import ModuleType
from typing import Self
from unittest.mock import _patch
from unittest.mock import patch

from attrs import define
from attrs import field


@define(slots=True, frozen=True)
class CommandsTester:
    """
    Helper class to simplify testing command discovery.

    Patches :func:`toolr._decorators._get_command_group_storage` so each
    test gets a fresh dict, then (in :meth:`discover`) imports modules
    under the search path's ``tools/`` package and loads ``toolr.commands``
    entry points to populate that dict. Tests then read it via
    :meth:`collected_command_groups`.
    """

    search_path: Path
    skip_loading_entry_points: bool = field(default=False, repr=False)
    sys_path: list[str] = field(init=False, repr=False)
    sys_modules: dict[str, ModuleType] = field(init=False, repr=False)
    command_group_collector: dict[str, object] = field(init=False, repr=False, factory=dict)
    command_group_patcher: _patch = field(init=False, repr=False)
    entry_points_patcher: _patch = field(init=False, repr=False)
    cwd: Path = field(init=False, repr=False, factory=Path.cwd)

    @sys_path.default
    def _default_sys_path(self) -> list[str]:
        return sys.path[:]

    @sys_modules.default
    def _default_sys_modules(self) -> dict[str, ModuleType]:
        # Copy sys.modules but exclude our testing thirdparty package and any local tools already imported
        return {
            name: sys.modules[name]
            for name in sys.modules
            if name not in ("tools", "thirdparty") and not name.startswith(("tools.", "thirdparty."))
        }

    @command_group_patcher.default
    def _default_command_group_patcher(self) -> _patch:
        return patch("toolr._decorators._get_command_group_storage", return_value=self.command_group_collector)

    @entry_points_patcher.default
    def _default_entry_points_patcher(self) -> _patch:
        return patch("importlib.metadata.entry_points", return_value=[])

    def collected_command_groups(self) -> dict[str, object]:
        """
        Get the collected command groups.
        """
        return {**self.command_group_collector}

    def discover(self) -> None:
        """Trigger Python-side discovery against ``search_path``.

        Drives the same import-and-walk pipeline that
        ``python -m toolr._introspect`` uses on behalf of the Rust binary.
        Registers ``tools/*.py`` modules and any installed
        ``toolr.commands`` entry points (unless the
        ``skip_loading_entry_points`` flag was set on construction).
        """
        # Imported lazily to avoid registering modules at testing.py import time.
        from toolr._introspect import _import_tools_modules  # noqa: PLC0415
        from toolr._introspect import _load_entry_points  # noqa: PLC0415

        warnings: list[str] = []
        _import_tools_modules(warnings)
        if not self.skip_loading_entry_points:
            _load_entry_points(warnings)

    def __enter__(self) -> Self:
        """
        Enter the context manager.
        """
        sys.modules.clear()
        sys.modules.update(self.sys_modules)
        os.chdir(self.search_path)
        if self.skip_loading_entry_points:
            self.entry_points_patcher.start()
        self.command_group_patcher.start()
        # Replace sys.path with the search path plus the site-packages
        # entries from the saved sys_path; drop anything that points
        # back at the host repo (which would shadow the fixture's
        # ``tools/`` tree with the repo's own).
        site_pkg_entries = [p for p in self.sys_path if "site-packages" in p or "dist-packages" in p]
        sys.path[:] = [str(self.search_path), *site_pkg_entries]
        return self

    def __exit__(self, *args: object) -> None:
        """
        Exit the context manager.
        """
        os.chdir(self.cwd)
        self.command_group_patcher.stop()
        if self.skip_loading_entry_points:
            self.entry_points_patcher.stop()
        self.command_group_collector.clear()
        sys.path[:] = self.sys_path
