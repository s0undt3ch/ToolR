"""
Utilities for testing ToolR commands and command-group discovery.

``CommandsTester`` patches the registry storage in
:mod:`toolr._decorators` and imports every module under the
``tools.*`` package so the command-group registry is populated, the
same way authors register commands at module import time. A module
that fails to import makes ``discover()`` raise an ``ExceptionGroup``.

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


def _import_tools_modules() -> list[Exception]:
    """Import every module under the top-level ``tools`` package.

    A module that fails to import doesn't stop the walk; every failure is
    returned, each with a note naming the module that raised it.
    """
    try:
        tools_pkg = importlib.import_module("tools")
    except Exception as exc:  # noqa: BLE001  # reported by the caller
        if isinstance(exc, ModuleNotFoundError) and exc.name == "tools":
            # No `tools/` package on sys.path; nothing to walk.
            return []
        exc.add_note("raised while importing `tools`")
        return [exc]

    errors: list[Exception] = []
    _import_package_modules(tools_pkg, errors)
    return errors


def _import_package_modules(package: ModuleType, errors: list[Exception]) -> None:
    """Import every module under ``package``, recursing into subpackages.

    ``pkgutil.walk_packages`` isn't used: it imports each subpackage itself,
    so a subpackage that raises anything but ``ImportError`` would escape and
    stop the walk, and its body would run a second time on our own import.
    A subpackage is only descended into once it imported cleanly.
    """
    search_paths = getattr(package, "__path__", None)
    if not search_paths:
        return
    for module_info in pkgutil.iter_modules(search_paths, prefix=f"{package.__name__}."):
        try:
            # `module_info.name` is enumerated by pkgutil from the local
            # `tools` package path, not user input — safe to import.
            module = importlib.import_module(
                module_info.name
            )  # nosemgrep: python.lang.security.audit.non-literal-import.non-literal-import
        except Exception as exc:  # noqa: BLE001  # we want every error
            exc.add_note(f"raised while importing `{module_info.name}`")
            errors.append(exc)
            continue
        if module_info.ispkg:
            _import_package_modules(module, errors)


def _isolated_sys_path(search_path: Path, saved: list[str]) -> list[str]:
    """Return ``saved`` with ``search_path`` first and no other ``tools`` provider.

    Drops the ``""`` (cwd) entry, ``search_path`` itself and any entry with a
    top-level ``tools`` package or module, which would shadow the fixture's
    ``tools``, or merge into it when the fixture's is a namespace package.
    Site-packages directories always stay. Everything else (the stdlib,
    editable installs, ``PYTHONPATH``) stays importable.
    """
    kept = []
    for entry in saved:
        if entry in ("", str(search_path)):
            continue
        is_site_packages = "site-packages" in entry or "dist-packages" in entry
        provides_tools = Path(entry, "tools").is_dir() or Path(entry, "tools.py").is_file()
        if provides_tools and not is_site_packages:
            continue
        kept.append(entry)
    return [str(search_path), *kept]


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

        Raises:
            ExceptionGroup: One or more ``tools`` modules failed to import,
                for example because a command has no docstring. Every
                module is imported first; the group holds each failure.
        """
        errors = _import_tools_modules()
        if errors:
            msg = (
                f"CommandsTester.discover(): {len(errors)} module(s) under "
                f"{self.search_path / 'tools'} failed to import"
            )
            raise ExceptionGroup(msg, errors)

    def __enter__(self) -> Self:
        """
        Enter the context manager.
        """
        sys.modules.clear()
        sys.modules.update(self.sys_modules)
        os.chdir(self.search_path)
        self.command_group_patcher.start()
        # Put the search path first and hide every other `tools` provider
        # (the host repo root among them): it would shadow the fixture's
        # `tools/`, or merge into it when that is a namespace package.
        sys.path[:] = _isolated_sys_path(self.search_path, self.sys_path)
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
