"""
Utilities for testing ToolR commands and command-group discovery.

``CommandsTester`` patches the registry storage in
:mod:`toolr._decorators` and imports every module under the
``tools.*`` package so the command-group registry is populated, the
same way authors register commands at module import time.

Usage: ``with CommandsTester(search_path=...) as t`` yields an
isolated collector available via ``t.collected_command_groups()``.
"""

from __future__ import annotations

import importlib
import os
import pkgutil
import sys
from pathlib import Path
from types import ModuleType
from typing import Self
from unittest.mock import _patch
from unittest.mock import patch

from attrs import define
from attrs import field


def _import_tools_modules(warnings: list[str]) -> None:
    """Import every module under the top-level ``tools`` package.

    Failures importing a single module are converted to a warning string
    and the walk continues — one bad file must not poison discovery.
    """
    try:
        tools_pkg = importlib.import_module("tools")
    except ModuleNotFoundError:
        # No `tools/` package on sys.path; nothing to walk.
        return
    except Exception as exc:  # noqa: BLE001  # pragma: no cover - defensive
        warnings.append(f"failed to import top-level `tools` package: {exc!r}")
        return

    search_paths = getattr(tools_pkg, "__path__", None)
    if not search_paths:
        return

    for module_info in pkgutil.walk_packages(search_paths, prefix="tools."):
        try:
            # `module_info.name` is enumerated by pkgutil from the local
            # `tools` package path, not user input — safe to import.
            importlib.import_module(
                module_info.name
            )  # nosemgrep: python.lang.security.audit.non-literal-import.non-literal-import
        except Exception as exc:  # noqa: BLE001  # we want every error
            warnings.append(f"failed to import `{module_info.name}`: {type(exc).__name__}: {exc}")


@define(slots=True, frozen=True)
class CommandsTester:
    """
    Helper class to simplify testing command discovery.

    Patches :func:`toolr._decorators._get_command_group_storage` so each
    test gets a fresh dict, then (in :meth:`discover`) imports modules
    under the search path's ``tools/`` package to populate that dict.
    Tests then read it via :meth:`collected_command_groups`.
    """

    search_path: Path
    sys_path: list[str] = field(init=False, repr=False)
    sys_modules: dict[str, ModuleType] = field(init=False, repr=False)
    command_group_collector: dict[str, object] = field(init=False, repr=False, factory=dict)
    command_group_patcher: _patch = field(init=False, repr=False)
    cwd: Path = field(init=False, repr=False, factory=Path.cwd)

    @sys_path.default
    def _default_sys_path(self) -> list[str]:
        return sys.path[:]

    @sys_modules.default
    def _default_sys_modules(self) -> dict[str, ModuleType]:
        # Copy sys.modules but exclude the example plugin package and any
        # local tools already imported, so the harness can reload them
        # cleanly on each test.
        return {
            name: sys.modules[name]
            for name in sys.modules
            if name not in ("tools", "toolr_example_plugin")
            and not name.startswith(("tools.", "toolr_example_plugin."))
        }

    @command_group_patcher.default
    def _default_command_group_patcher(self) -> _patch:
        return patch(
            "toolr._decorators._get_command_group_storage",
            return_value=self.command_group_collector,
        )

    def collected_command_groups(self) -> dict[str, object]:
        """
        Get the collected command groups.
        """
        return {**self.command_group_collector}

    def discover(self) -> None:
        """Trigger Python-side discovery against ``search_path``.

        Imports every module under the ``tools.*`` package so the
        command-group registry is populated, the same way authors
        register commands at module import time.
        """
        warnings: list[str] = []
        _import_tools_modules(warnings)

    def __enter__(self) -> Self:
        """
        Enter the context manager.
        """
        sys.modules.clear()
        sys.modules.update(self.sys_modules)
        os.chdir(self.search_path)
        self.command_group_patcher.start()
        # Replace sys.path with the search path plus the site-packages
        # entries from the saved sys_path; drop anything that points
        # back at the host repo (which would shadow the fixture's
        # ``tools/`` tree with the repo's own).
        site_pkg_entries = [
            p for p in self.sys_path if "site-packages" in p or "dist-packages" in p
        ]
        sys.path[:] = [str(self.search_path), *site_pkg_entries]
        return self

    def __exit__(self, *args: object) -> None:
        """
        Exit the context manager.
        """
        os.chdir(self.cwd)
        self.command_group_patcher.stop()
        self.command_group_collector.clear()
        sys.path[:] = self.sys_path
        # Reverse the module table back to the filtered snapshot `__enter__`
        # installed (real modules minus the volatile `tools` /
        # `toolr_example_plugin` packages). This undoes both the bare
        # `clear()` on enter and any modules imported inside the block, so a
        # long-lived process keeps its real imports instead of being left
        # wiped. We deliberately do NOT reinstate `tools` /
        # `toolr_example_plugin`: they are the reload-per-test targets, and
        # carrying a stale copy forward would pollute later tests.
        sys.modules.clear()
        sys.modules.update(self.sys_modules)
