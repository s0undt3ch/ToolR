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
from importlib.machinery import ModuleSpec
from importlib.machinery import PathFinder
from pathlib import Path
from types import ModuleType
from typing import TYPE_CHECKING
from typing import Any
from typing import Self
from unittest.mock import _patch
from unittest.mock import patch

from attrs import define
from attrs import field

from toolr._exc import ToolrError
from toolr.testing._duplicates import _format_duplicate_commands
from toolr.testing._shapes import _format_unsupported_commands
from toolr.testing._shapes import _unsupported_commands
from toolr.testing._unknown_groups import _escape_control_chars
from toolr.testing._unknown_groups import _format_unknown_groups

if TYPE_CHECKING:
    from toolr._decorators import DuplicateCommand
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


def _import_tools_modules(root: Path) -> list[Exception]:
    """Import every module under the top-level ``tools`` package.

    Only directories under ``root`` (the fixture's ``tools/``) are walked, so
    a ``__path__`` that ``pkgutil.extend_path`` filled from ``sys.path`` can't
    pull in another ``tools/``. A module that fails to import doesn't stop
    the walk; every failure is returned, each with a note naming the module
    that raised it.
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
    _import_package_modules(tools_pkg, errors, set(), root.resolve())
    return errors


def _import_package_modules(
    package: ModuleType, errors: list[Exception], seen: set[Path], root: Path
) -> None:
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
    Directories outside ``root`` are skipped.
    """
    for directory in getattr(package, "__path__", ()):
        resolved = Path(directory).resolve()
        if resolved in seen or not resolved.is_relative_to(root):
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
                _import_package_modules(module, errors, seen, root)


def _fixture_only_namespace_path(spec: ModuleSpec, locations: list[str]) -> None:
    """Make a namespace ``spec``'s ``__path__`` recompute from ``locations`` only.

    A namespace ``__path__`` recomputes itself from all of ``sys.path`` whenever
    ``sys.path`` or the import caches change. Keep importlib's own type
    (``importlib.resources`` rejects any other) but give it a path finder that
    only looks in ``locations`` and hands back a plain list, which can't
    recompute itself.
    """
    # Private importlib API: `importlib._bootstrap_external._NamespacePath`
    # (also `NamespacePath` in 3.15) takes `(name, path, path_finder)` on CPython
    # 3.11-3.15; `test_namespace_tools_path_stays_in_the_fixture_when_sys_path_changes`
    # and `test_namespace_tools_supports_importlib_resources` guard it.

    def path_finder(fullname: str, _parent_path: object) -> ModuleSpec | None:
        found = PathFinder.find_spec(fullname, locations)
        if found is not None and found.loader is None:
            found.submodule_search_locations = list(found.submodule_search_locations or ())
        return found

    namespace_path: Any = spec.submodule_search_locations
    spec.submodule_search_locations = type(namespace_path)(
        spec.name, list(namespace_path), path_finder
    )


class _FixtureToolsFinder:
    """Resolve the top-level ``tools`` package from the fixture only.

    Sits first on ``sys.meta_path`` while a :class:`CommandsTester` is active,
    so a ``tools`` package or module anywhere else on ``sys.path`` (the host
    repo root, a site directory with a stray ``tools``) can't replace the
    fixture's, or merge into it when the fixture's is a namespace package.
    Every ``sys.path`` entry stays, so the rest of what lives there stays
    importable. Submodules need no help: they resolve through
    ``tools.__path__``, which only names the fixture. When the fixture has
    no ``tools``, ``find_spec`` raises ``ModuleNotFoundError`` instead of
    returning ``None``, because raising is what stops later finders.
    """

    def __init__(self, search_path: Path) -> None:
        self._locations = [str(search_path)]

    def find_spec(
        self, fullname: str, _path: object = None, target: ModuleType | None = None
    ) -> ModuleSpec | None:
        if fullname != "tools":
            return None
        spec = PathFinder.find_spec(fullname, self._locations, target)
        if spec is None:
            msg = f"No module named {fullname!r}"
            raise ModuleNotFoundError(msg, name=fullname)
        if spec.loader is None:
            _fixture_only_namespace_path(spec, self._locations)
        return spec


def _tester_sys_path(search_path: Path, saved: list[str]) -> list[str]:
    """Return ``saved`` with ``search_path`` first and without the ``""`` (cwd) entry.

    The cwd is ``search_path`` while the tester is active, so ``""`` would
    only repeat it. Which ``tools`` gets imported is up to
    :class:`_FixtureToolsFinder`, not ``sys.path``.
    """
    return [str(search_path), *(entry for entry in saved if entry not in ("", str(search_path)))]


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
    duplicate_command_collector: list[DuplicateCommand] = field(
        init=False, repr=False, factory=list
    )
    duplicate_command_patcher: _patch = field(init=False, repr=False)
    cwd: Path = field(init=False, repr=False, factory=Path.cwd)
    tools_finder: _FixtureToolsFinder = field(init=False, repr=False)

    @tools_finder.default
    def _default_tools_finder(self) -> _FixtureToolsFinder:
        return _FixtureToolsFinder(self.search_path)

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

    @duplicate_command_patcher.default
    def _default_duplicate_command_patcher(self) -> _patch:
        # Unpatched, `_get_duplicate_command_storage` keeps nothing.
        return patch(
            "toolr._decorators._get_duplicate_command_storage",
            return_value=self.duplicate_command_collector,
        )

    def collected_command_groups(self) -> dict[str, object]:
        """
        Get the collected command groups.

        Raises:
            ValueError: A command under ``tools/`` decorates a nested
                ``def`` or passes a non-literal ``group=`` or name; a
                standalone ``@command(group=...)`` names a group that no
                ``command_group(...)`` declares, or passes ``group=""``; or
                two functions define the same command in a group. The
                message is the one the manifest build gives, which fails
                the same way.
        """
        tools_dir = self.search_path / "tools"
        # Checked first, as the manifest build does: these hide what the others report.
        unsupported = _unsupported_commands(tools_dir)
        if unsupported:
            raise ValueError(_format_unsupported_commands(unsupported))
        if self.pending_command_collector:
            raise ValueError(
                _format_unknown_groups(
                    self.pending_command_collector,
                    self.command_group_collector,
                    tools_dir,
                )
            )
        if self.duplicate_command_collector:
            raise ValueError(
                _format_duplicate_commands(
                    self.duplicate_command_collector, self.search_path / "tools"
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
        errors = _import_tools_modules(self.search_path / "tools")
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
        self.duplicate_command_patcher.start()
        self.command_group_patcher.start()
        sys.path[:] = _tester_sys_path(self.search_path, self.sys_path)
        # Only the fixture may provide `tools`; see `_FixtureToolsFinder`.
        sys.meta_path.insert(0, self.tools_finder)
        return self

    def __exit__(self, *args: object) -> None:
        """
        Exit the context manager.
        """
        sys.meta_path.remove(self.tools_finder)
        self.command_group_patcher.stop()
        self.command_group_collector.clear()
        self.pending_command_patcher.stop()
        self.pending_command_collector.clear()
        self.duplicate_command_patcher.stop()
        self.duplicate_command_collector.clear()
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
        # Last, so the steps above still run if the old cwd is gone.
        os.chdir(self.cwd)
