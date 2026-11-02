"""
Utilities for testing ToolR commands and command-group discovery.

``CommandsTester`` patches the registry storage in
:mod:`toolr._decorators` and imports every module under the
``tools.*`` package so the command-group registry is populated, the
same way authors register commands at module import time. A module
that fails to import makes ``discover()`` raise an ``ExceptionGroup``.
A standalone ``@command(group=...)`` whose group no module declares makes
``collected_command_groups()`` raise ``ValueError``.

Usage: ``with CommandsTester(search_path=...) as t`` yields an
isolated collector available via ``t.collected_command_groups()``.
"""

from __future__ import annotations

import importlib
import os
import site
import sys
import sysconfig
from pathlib import Path
from types import ModuleType
from typing import TYPE_CHECKING
from typing import Self
from unittest.mock import _patch
from unittest.mock import patch

from attrs import define
from attrs import field

from toolr._exc import ToolrError
from toolr.testing._unknown_groups import _escape_control_chars
from toolr.testing._unknown_groups import _format_unknown_groups

if TYPE_CHECKING:
    from toolr._decorators import PendingCommand


def _is_pruned_dir(name: str) -> bool:
    """Whether the ``tools/`` walks skip a directory called ``name``.

    Mirrors ``is_pruned_dir`` in ``crates/toolr-core/src/hash.rs``; change
    both together.
    """
    return name.startswith(".") or name == "__pycache__"


def _reject_symlinked_modules(tools_dir: Path) -> None:
    """Raise if any ``*.py`` under ``tools_dir`` is a symlink.

    Mirrors ``list_python_files`` in ``crates/toolr-core/src/hash.rs``;
    change both together. Dot-directories and ``__pycache__`` are pruned and
    symlinked directories, ``.py``-named or not, are not followed, so none of
    them is checked.

    Raises:
        ToolrError: With the same text ``toolr`` prints for the tree.
    """
    links: list[Path] = []
    pending = [tools_dir]
    while pending:
        directory = pending.pop()
        try:
            with os.scandir(directory) as it:
                entries = list(it)
        except OSError:
            continue
        for entry in entries:
            if entry.is_dir(follow_symlinks=False):
                if not _is_pruned_dir(entry.name):
                    pending.append(Path(entry.path))
            elif (
                entry.is_symlink()
                and os.path.splitext(entry.name)[1] == ".py"
                # A `.py`-named link to a directory is a directory link. A
                # target that can't be stat'd (loop, unreadable) still counts.
                and not os.path.isdir(entry.path)
            ):
                links.append(Path(entry.path))
    if not links:
        return
    base = tools_dir.parent
    paths = sorted(link.relative_to(base) for link in links)
    details = "\n".join(f"  - {_escape_control_chars(path.as_posix())}" for path in paths)
    root = _escape_control_chars(tools_dir.name)
    msg = (
        f"symlinked modules are not supported ({len(paths)}):\n{details}\n"
        f"Put the file itself under {root}/, or share it as a Python package."
    )
    raise ToolrError(msg)


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
    _import_package_modules(tools_pkg, errors, set())
    return errors


def _import_package_modules(package: ModuleType, errors: list[Exception], seen: set[Path]) -> None:
    """Import every module under ``package``, recursing into subdirectories.

    Mirrors the manifest build's walk in
    ``crates/toolr-core/src/hash.rs`` (``list_python_files`` and
    ``is_pruned_dir``); change both together. Every ``.py`` file and every
    subdirectory, with or without ``__init__.py``, is visited except
    dot-directories, ``__pycache__`` and symlinks (``discover()`` has already
    rejected ``.py`` links). Names that aren't Python
    identifiers can't be imported and are skipped. A subdirectory is only
    descended into once its own import succeeded, and each directory is
    walked once, so a ``__path__`` pointing back at a parent can't loop.
    """
    for directory in getattr(package, "__path__", ()):
        resolved = Path(directory).resolve()
        if resolved in seen:
            continue
        seen.add(resolved)
        try:
            with os.scandir(directory) as it:
                entries = sorted(it, key=lambda entry: entry.name)
        except OSError:
            continue
        for entry in entries:
            if entry.is_symlink():
                continue
            if entry.is_dir():
                if _is_pruned_dir(entry.name) or not entry.name.isidentifier():
                    continue
                is_package = True
                stem = entry.name
            elif entry.name.endswith(".py") and entry.name != "__init__.py":
                stem = entry.name.removesuffix(".py")
                if not stem.isidentifier():
                    continue
                is_package = False
            else:
                continue
            name = f"{package.__name__}.{stem}"
            try:
                # `name` comes from the local `tools` directory listing, not
                # user input — safe to import.
                module = importlib.import_module(
                    name
                )  # nosemgrep: python.lang.security.audit.non-literal-import.non-literal-import
            except Exception as exc:  # noqa: BLE001  # we want every error
                exc.add_note(f"raised while importing `{name}`")
                errors.append(exc)
                continue
            if is_package:
                _import_package_modules(module, errors, seen)


def _provides_tools(entry: str) -> bool:
    """Whether ``entry`` holds a ``tools/`` directory or ``tools.py``.

    Matches the exact name, so ``Tools/`` doesn't count on a case-insensitive
    filesystem (the import system wouldn't use it either).
    """
    try:
        with os.scandir(entry) as it:
            return any(
                (child.name == "tools" and child.is_dir())
                or (child.name == "tools.py" and child.is_file())
                for child in it
            )
    except OSError:
        return False


def _normalise(path: str) -> str:
    """Return ``path`` resolved and case-folded the way the OS compares paths."""
    return os.path.normcase(os.path.realpath(path))


def _site_dirs() -> frozenset[str]:
    """Return the running interpreter's own site directories, normalised.

    Asks the interpreter rather than matching names, so a repo checked out
    under a directory such as ``windows-site-packages`` isn't mistaken for one.
    """
    paths = sysconfig.get_paths()
    return frozenset(
        _normalise(path)
        for path in (
            *site.getsitepackages(),
            site.getusersitepackages(),
            paths["purelib"],
            paths["platlib"],
        )
    )


def _isolated_sys_path(search_path: Path, saved: list[str]) -> list[str]:
    """Return ``saved`` with ``search_path`` first and no other ``tools`` provider.

    Drops the ``""`` (cwd) entry, ``search_path`` itself and any entry with a
    top-level ``tools`` package or module, because such an entry (the host
    repo root among them) would stand in for a fixture without ``tools/``,
    and when the fixture's ``tools/`` is a namespace package, a regular
    ``tools`` package or ``tools.py`` elsewhere replaces it and another
    namespace ``tools/`` merges into it. The interpreter's own site
    directories (see ``_site_dirs``) always stay, whatever they hold; a
    directory that only has ``site-packages`` in its name gets no such
    exemption. Everything else (the stdlib, src-layout editable installs,
    ``PYTHONPATH``) stays importable; an editable-install root that itself
    holds ``tools/`` is dropped too.
    """
    site_dirs = _site_dirs()
    kept = []
    for entry in saved:
        if entry in ("", str(search_path)):
            continue
        if _provides_tools(entry) and _normalise(entry) not in site_dirs:
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

    A standalone ``@command`` whose ``group=`` names no declared group, or
    that has no ``group=``, makes :meth:`collected_command_groups` raise
    ``ValueError`` with the message the manifest build gives.
    """

    search_path: Path
    sys_path: list[str] = field(init=False, repr=False)
    sys_modules: dict[str, ModuleType] = field(init=False, repr=False)
    command_group_collector: dict[str, object] = field(init=False, repr=False, factory=dict)
    command_group_patcher: _patch = field(init=False, repr=False)
    pending_command_collector: list[PendingCommand] = field(init=False, repr=False, factory=list)
    pending_command_patcher: _patch = field(init=False, repr=False)
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

    @pending_command_patcher.default
    def _default_pending_command_patcher(self) -> _patch:
        # Standalone `@command`s wait here until their group is declared;
        # unpatched, `_get_pending_command_storage` keeps nothing.
        return patch(
            "toolr._decorators._get_pending_command_storage",
            return_value=self.pending_command_collector,
        )

    def collected_command_groups(self) -> dict[str, object]:
        """
        Get the collected command groups.

        Raises:
            ValueError: A standalone ``@command(group=...)`` names a group
                that no ``command_group(...)`` declares, or passes
                ``group=""``. The message is the one the manifest build
                gives, which fails the same way.
        """
        if self.pending_command_collector:
            raise ValueError(
                _format_unknown_groups(
                    self.pending_command_collector,
                    self.command_group_collector,
                    self.search_path / "tools",
                )
            )
        return {**self.command_group_collector}

    def discover(self) -> None:
        """Trigger Python-side discovery against ``search_path``.

        Imports every module under the ``tools.*`` package so the
        command-group registry is populated, the same way authors
        register commands at module import time.

        Raises:
            ToolrError: A ``*.py`` under ``tools/`` is a symlink, which
                ``toolr`` refuses too. Raised before anything is imported.
            ExceptionGroup: One or more ``tools`` modules failed to import,
                for example because a command has no docstring. Every
                module is imported first; the group holds each failure.
        """
        _reject_symlinked_modules(self.search_path / "tools")
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
        self.pending_command_patcher.start()
        self.command_group_patcher.start()
        # See `_isolated_sys_path` for why other `tools` providers are hidden.
        sys.path[:] = _isolated_sys_path(self.search_path, self.sys_path)
        return self

    def __exit__(self, *args: object) -> None:
        """
        Exit the context manager.
        """
        os.chdir(self.cwd)
        self.command_group_patcher.stop()
        self.command_group_collector.clear()
        self.pending_command_patcher.stop()
        self.pending_command_collector.clear()
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
