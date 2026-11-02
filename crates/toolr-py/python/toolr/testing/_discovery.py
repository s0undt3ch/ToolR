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
import sys
from collections.abc import Callable
from pathlib import Path
from types import ModuleType
from typing import Any
from typing import Self
from unittest.mock import _patch
from unittest.mock import patch

from attrs import define
from attrs import field

from toolr._decorators import PendingCommand
from toolr._decorators import _group_path


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
    dot-directories, ``__pycache__`` and symlinks. Names that aren't Python
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
                if (
                    entry.name.startswith(".")
                    or entry.name == "__pycache__"
                    or not entry.name.isidentifier()
                ):
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


def _isolated_sys_path(search_path: Path, saved: list[str]) -> list[str]:
    """Return ``saved`` with ``search_path`` first and no other ``tools`` provider.

    Drops the ``""`` (cwd) entry, ``search_path`` itself and any entry with a
    top-level ``tools`` package or module, because such an entry (the host
    repo root among them) would stand in for a fixture without ``tools/``,
    and when the fixture's ``tools/`` is a namespace package, a regular
    ``tools`` package or ``tools.py`` elsewhere replaces it and another
    namespace ``tools/`` merges into it. Site-packages directories always
    stay. Everything else (the stdlib,
    src-layout editable installs, ``PYTHONPATH``) stays importable; an
    editable-install root that itself holds ``tools/`` is dropped too.
    """
    kept = []
    for entry in saved:
        if entry in ("", str(search_path)):
            continue
        is_site_packages = "site-packages" in entry or "dist-packages" in entry
        provides_tools = _provides_tools(entry)
        if provides_tools and not is_site_packages:
            continue
        kept.append(entry)
    return [str(search_path), *kept]


def _is_control(char: str) -> bool:
    """C0, DEL and C1 controls, as ``is_control`` in ``crates/toolr-core/src/text.rs``."""
    code = ord(char)
    return code <= 0x1F or 0x7F <= code <= 0x9F


def _is_invisible_format(char: str) -> bool:
    """Zero-width and bidi format characters, as ``is_invisible_format`` in ``text.rs``."""
    code = ord(char)
    return (
        code == 0x061C
        or 0x200B <= code <= 0x200F
        or 0x202A <= code <= 0x202E
        or 0x2060 <= code <= 0x2064
        or 0x2066 <= code <= 0x2069
        or code == 0xFEFF
    )


def _escape_control_chars(text: str) -> str:
    r"""Show control and invisible format characters as ``\u{..}``, like ``escape_control_chars``."""
    return "".join(
        f"\\u{{{ord(char):x}}}" if _is_control(char) or _is_invisible_format(char) else char
        for char in text
    )


def _edit_distance(a: str, b: str) -> int:
    """Levenshtein distance, as ``edit_distance`` in the manifest build."""
    previous = list(range(len(b) + 1))
    for i, char_a in enumerate(a, start=1):
        current = [i]
        for j, char_b in enumerate(b, start=1):
            cost = 0 if char_a == char_b else 1
            current.append(min(previous[j] + 1, current[j - 1] + 1, previous[j - 1] + cost))
        previous = current
    return previous[-1]


def _nearest_group(target: str, registered: list[str]) -> str | None:
    """The closest declared group to ``target``, as ``nearest_group`` in the manifest build.

    The threshold uses the UTF-8 length, as Rust's ``str::len`` does.
    ``registered`` is sorted, so among equally close groups the first
    one wins; the manifest build picks one of them in hash order.
    """
    limit = max(len(target.encode()) // 3, 2)
    best: tuple[int, str] | None = None
    for candidate in registered:
        distance = _edit_distance(target, candidate)
        if distance <= limit and (best is None or distance < best[0]):
            best = (distance, candidate)
    return None if best is None else best[1]


def _unknown_group_line(group: str, module: str, command: str, registered: list[str]) -> str:
    """One line of the unknown-group error, worded as ``format_unknown_groups`` in Rust."""
    location = f"{_escape_control_chars(module)}::{_escape_control_chars(command)}"
    if not group:
        return f"  - {location}: `@command` is missing a `group=...` kwarg."
    line = (
        f"  - {location}: references group `{_escape_control_chars(group)}` which has no "
        "`command_group(...)` declaration."
    )
    suggestion = _nearest_group(group, registered)
    if suggestion is not None:
        line += f" Did you mean `{_escape_control_chars(suggestion)}`?"
    return line


def _source_order_key(func: Callable[..., Any], tools_dir: Path) -> tuple[int, tuple[str, ...]]:
    """Sort key for a command's file, matching the manifest build's sorted walk.

    Files under ``tools_dir`` sort by path components, as Rust sorts its
    ``PathBuf`` list in ``list_python_files``. Functions defined elsewhere
    (a test body, say) sort after them; the stable sort keeps their order.
    """
    try:
        relative = Path(func.__code__.co_filename).relative_to(tools_dir)
    except ValueError:
        return (1, ())
    return (0, relative.parts)


@define(slots=True, frozen=True)
class CommandsTester:
    """
    Helper class to simplify testing command discovery.

    Patches :func:`toolr._decorators._get_command_group_storage` so each
    test gets a fresh dict, then (in :meth:`discover`) imports modules
    under the search path's ``tools/`` package to populate that dict.
    Tests then read it via :meth:`collected_command_groups`.

    It also patches :func:`toolr._decorators._get_pending_command_storage`
    with its own list, which holds standalone ``@command(group=...)``
    commands until their group is declared. Entries still there when
    :meth:`collected_command_groups` is called name groups no module
    declares, and make it raise ``ValueError``.
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
            registered = sorted(_group_path(name) for name in self.command_group_collector)
            tools_dir = self.search_path / "tools"
            pending = sorted(
                self.pending_command_collector,
                key=lambda entry: _source_order_key(entry.func, tools_dir),
            )
            lines = [
                _unknown_group_line(entry.group, entry.func.__module__, entry.name, registered)
                for entry in pending
            ]
            err_msg = "\n".join([f"unknown group references ({len(lines)}):", *lines])
            raise ValueError(err_msg)
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
