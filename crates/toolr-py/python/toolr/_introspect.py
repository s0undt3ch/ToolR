"""Dynamic manifest introspection helper.

Invoked as ``python -m toolr._introspect`` from the Rust side inside the
project's tools venv. Walks the ``command_group`` registry after
importing every module under ``tools.*``, and writes a JSON payload
to stdout.

The wire format is defined in ``specs/archive/2026/rust-front-end/07-plan-6-dynamic-manifest.md``.
Bump ``PAYLOAD_SCHEMA_VERSION`` on every breaking change.
"""

from __future__ import annotations

import argparse
import importlib
import inspect
import json
import os
import pkgutil
import sys
from typing import Any

PAYLOAD_SCHEMA_VERSION = 1


def _ensure_tools_on_syspath(tools_root: str | None) -> None:
    """Insert the parent of ``tools_root`` on ``sys.path`` so ``import tools.<sub>`` works."""
    if not tools_root:
        return
    parent = os.path.dirname(os.path.abspath(tools_root))
    if parent and parent not in sys.path:
        sys.path.insert(0, parent)


def _import_tools_modules(warnings: list[str]) -> None:
    """Import every module under the top-level ``tools`` package.

    Failures importing a single module are converted to a warning string and
    the walk continues - one bad file must not poison the whole rebuild.
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
            importlib.import_module(module_info.name)
        except Exception as exc:  # noqa: BLE001  # we want every error
            warnings.append(f"failed to import `{module_info.name}`: {type(exc).__name__}: {exc}")


def _walk_registry() -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Read groups and commands from the toolr registry singleton."""
    from toolr._decorators import _get_command_group_storage  # noqa: PLC0415

    storage = _get_command_group_storage()
    groups: list[dict[str, Any]] = []
    commands: list[dict[str, Any]] = []

    for full_name, group in storage.items():
        # CommandGroup.full_name is "tools.<name>" or "tools.<parent>.<name>";
        # strip the leading "tools." then split on the final '.' to
        # separate the leaf group name from its (possibly multi-level)
        # parent path.
        display_path = full_name.removeprefix("tools.")
        leaf, parent_path = _split_leaf(display_path)
        groups.append(
            {
                "name": leaf,
                "title": group.title,
                "description": group.description or "",
                "parent": parent_path,
                "origin": "dynamic",
            }
        )
        for cmd_name, func in group.get_commands().items():
            commands.append(_command_entry(display_path, cmd_name, func))

    return groups, commands


def _split_leaf(path: str) -> tuple[str, str | None]:
    """Split a dotted ``parent.child`` path into ``(leaf, parent_path)``.

    Top-level groups have no `.`, so `parent_path` is ``None``.
    """
    if "." not in path:
        return path, None
    parent, _, leaf = path.rpartition(".")
    return leaf, parent


def _command_entry(group_name: str, cmd_name: str, func: Any) -> dict[str, Any]:
    """Serialize a single registered command function."""
    module = getattr(func, "__module__", "") or ""
    function = getattr(func, "__name__", cmd_name)
    doc = inspect.getdoc(func) or ""
    summary, _, description = doc.partition("\n\n")
    return {
        "name": cmd_name,
        "group": group_name,
        "module": module,
        "function": function,
        "summary": summary.strip(),
        "description": description.strip(),
        # Argument extraction is intentionally omitted here. The static
        # parser already emits these for `tools/*.py` files; the dynamic
        # layer only adds *missing* commands.
        "arguments": [],
        "imports": [],
        "origin": "dynamic",
    }


def build_payload(tools_root: str | None) -> dict[str, Any]:
    warnings: list[str] = []
    _ensure_tools_on_syspath(tools_root)
    _import_tools_modules(warnings)
    groups, commands = _walk_registry()
    return {
        "payload_schema_version": PAYLOAD_SCHEMA_VERSION,
        "groups": groups,
        "commands": commands,
        "warnings": warnings,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="toolr._introspect",
        description="Dump toolr dynamic-layer manifest as JSON to stdout.",
    )
    parser.add_argument(
        "--tools-root",
        default=None,
        help="Absolute path to the project's tools/ directory.",
    )
    args = parser.parse_args(argv)
    payload = build_payload(args.tools_root)
    json.dump(payload, sys.stdout, separators=(",", ":"))
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
