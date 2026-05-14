"""Build a toolr manifest fragment for a third-party package.

Walks the package's `command_group` / `@group.command` registry to
produce a static `toolr-manifest.json` that the Rust binary can
discover and merge without any further Python introspection.
"""

from __future__ import annotations

import argparse
import importlib
import json
import sys
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from typing import get_args
from typing import get_origin

from toolr._registry import MANIFEST_SCHEMA_VERSION
from toolr._registry import _get_command_group_storage
from toolr.utils._signature import Arg
from toolr.utils._signature import KwArg
from toolr.utils._signature import VarArg
from toolr.utils._signature import get_signature


@dataclass(frozen=True)
class BuildResult:
    """Result of `build_manifest`."""

    fragment: dict[str, Any]
    output_path: Path
    drift: bool = False
    """True only when `check=True` and the regenerated fragment differs
    from the file currently on disk."""


class BuildManifestError(Exception):
    """Raised when the manifest cannot be built (no commands, etc.)."""


_ALLOWED_ARG_KINDS = {"positional", "optional", "flag"}


def _validate_fragment(fragment: dict[str, Any]) -> None:
    """Defensive schema check. Catches author-side packaging mistakes."""
    version = fragment.get("toolr_schema_version")
    if not isinstance(version, int) or version < 1:
        err_msg = f"`toolr_schema_version` must be a positive int, got {version!r}"
        raise BuildManifestError(err_msg)
    if not isinstance(fragment.get("package"), str):
        err_msg = "`package` must be a string"
        raise BuildManifestError(err_msg)
    for group in fragment.get("groups", []):
        if not isinstance(group.get("name"), str):
            err_msg = f"group missing `name`: {group!r}"
            raise BuildManifestError(err_msg)
    for cmd in fragment.get("commands", []):
        for key in ("name", "group", "module", "function"):
            if not isinstance(cmd.get(key), str):
                err_msg = f"command missing required string field `{key}`: {cmd!r}"
                raise BuildManifestError(err_msg)
        for arg_entry in cmd.get("arguments", []):
            if arg_entry.get("kind") not in _ALLOWED_ARG_KINDS:
                err_msg = (
                    f"argument `{arg_entry.get('name')!r}` has invalid kind "
                    f"`{arg_entry.get('kind')}` - must be one of {_ALLOWED_ARG_KINDS}"
                )
                raise BuildManifestError(err_msg)


def build_manifest(
    package_name: str,
    *,
    output_path: Path | None = None,
    schema_version: int | None = None,
    check: bool = False,
) -> BuildResult:
    """Generate a manifest fragment for `package_name`.

    Args:
        package_name: Dotted name of the package to introspect.
        output_path: Where to write `toolr-manifest.json`. Defaults to
            ``<package_dir>/toolr-manifest.json``.
        schema_version: Override the schema version written out.
        check: If True, do not write the file; instead, compare the
            generated fragment against the file currently at
            `output_path`. Sets ``BuildResult.drift=True`` on mismatch.

    Raises:
        ModuleNotFoundError: `package_name` is not importable.
        BuildManifestError: The package has no toolr commands.
    """
    module = importlib.import_module(package_name)
    package_root = _resolve_package_root(module, package_name)
    if output_path is None:
        output_path = package_root / "toolr-manifest.json"
    version = schema_version if schema_version is not None else MANIFEST_SCHEMA_VERSION

    fragment = _collect_fragment(package_name, version)
    if not fragment["groups"] and not fragment["commands"]:
        err_msg = f"package `{package_name}` declares no toolr commands - nothing to write"
        raise BuildManifestError(err_msg)
    _validate_fragment(fragment)

    serialized = json.dumps(fragment, indent=2, sort_keys=True) + "\n"

    if check:
        existing = output_path.read_text() if output_path.is_file() else ""
        drift = existing != serialized
        return BuildResult(fragment=fragment, output_path=output_path, drift=drift)

    output_path.write_text(serialized)
    return BuildResult(fragment=fragment, output_path=output_path)


def _resolve_package_root(module: Any, package_name: str) -> Path:
    file = getattr(module, "__file__", None)
    if file is None:
        err_msg = (
            f"`{package_name}` has no `__file__` - cannot resolve its "
            "installed directory. Namespace packages are not supported."
        )
        raise BuildManifestError(err_msg)
    return Path(file).resolve().parent


def _collect_fragment(package_name: str, version: int) -> dict[str, Any]:
    """Walk the global registry, keep only groups/commands that belong to `package_name`."""
    storage = _get_command_group_storage()
    groups: list[dict[str, Any]] = []
    commands: list[dict[str, Any]] = []
    seen_groups: set[str] = set()

    for group in sorted(storage.values(), key=lambda g: g.full_name):
        for cmd_name, func in group.get_commands().items():
            if not _belongs_to_package(func, package_name):
                continue
            if group.name not in seen_groups:
                seen_groups.add(group.name)
                groups.append(
                    {
                        "name": group.name,
                        "title": group.title,
                        "description": group.description or "",
                    }
                )
            commands.append(_serialize_command(group.name, cmd_name, func))

    return {
        "toolr_schema_version": version,
        "package": package_name,
        "groups": groups,
        "commands": commands,
    }


def _belongs_to_package(func: Callable[..., Any], package_name: str) -> bool:
    module = getattr(func, "__module__", "")
    return module == package_name or module.startswith(f"{package_name}.")


def _serialize_command(group: str, name: str, func: Callable[..., Any]) -> dict[str, Any]:
    signature = get_signature(func)
    arguments = [_serialize_argument(arg) for arg in signature.arguments]
    return {
        "name": name,
        "group": group,
        "module": func.__module__,
        "function": func.__name__,
        "summary": signature.short_description or "",
        "description": signature.long_description or "",
        "arguments": arguments,
        # `imports` is filled in by the Rust static parser; left empty here.
        "imports": [],
    }


def _serialize_argument(arg: Arg | KwArg | VarArg) -> dict[str, Any]:
    """Map a `Signature.arguments` entry to the JSON fragment shape."""
    kind = _argument_kind(arg)
    return {
        "name": arg.name,
        "kind": kind,
        "help": arg.description or "",
        "default": _serialize_default(arg.default),
        "type_annotation": _serialize_type(arg.type),
        "allowed_values": [str(c) for c in (arg.choices or [])],
    }


def _argument_kind(arg: Arg | KwArg | VarArg) -> str:
    if isinstance(arg, KwArg):
        # store_true / store_false → bool flag with no value.
        if arg.action in ("store_true", "store_false"):
            return "flag"
        return "optional"
    # VarArg is a positional with nargs="*"/"+"; treat as positional.
    return "positional"


def _serialize_default(default: Any) -> str | None:
    if default is None:
        return None
    return repr(default)


def _serialize_type(annotation: Any) -> str | None:
    """Best-effort stringification of a Python type annotation."""
    if annotation is None:
        return None
    # typing.Literal["a", "b"] → "Literal"
    origin = get_origin(annotation)
    if origin is not None:
        # Preserve the Literal[...] tag so the Rust side recognises
        # allowed_values-bearing args.
        return getattr(origin, "__name__", None) or str(origin).rsplit(".", 1)[-1]
    name = getattr(annotation, "__name__", None)
    if name is not None:
        return name
    return str(annotation)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="python -m toolr.build",
        description=(
            "Generate toolr-manifest.json for a third-party command "
            "package by introspecting its command_group / @group.command "
            "declarations."
        ),
    )
    parser.add_argument("package", help="Dotted package name to introspect.")
    parser.add_argument(
        "--output",
        type=Path,
        default=None,
        help="Where to write the manifest. Defaults to <package-dir>/toolr-manifest.json.",
    )
    parser.add_argument(
        "--schema-version",
        type=int,
        default=None,
        help=f"Pin the schema version. Defaults to {MANIFEST_SCHEMA_VERSION}.",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="Don't write; exit 2 if the on-disk manifest differs from regenerated.",
    )
    parser.add_argument(
        "--quiet",
        action="store_true",
        help="Suppress informational output.",
    )
    args = parser.parse_args(argv)

    try:
        result = build_manifest(
            args.package,
            output_path=args.output,
            schema_version=args.schema_version,
            check=args.check,
        )
    except ModuleNotFoundError as exc:
        print(f"toolr.build: cannot import package: {exc}", file=sys.stderr)  # noqa: T201
        return 1
    except BuildManifestError as exc:
        print(f"toolr.build: {exc}", file=sys.stderr)  # noqa: T201
        return 1

    if args.check:
        if result.drift:
            print(  # noqa: T201
                f"toolr.build: {result.output_path} is out of date - "
                f"regenerate with `python -m toolr.build {args.package}`",
                file=sys.stderr,
            )
            return 2
        if not args.quiet:
            print(f"toolr.build: {result.output_path} is up to date.")  # noqa: T201
        return 0

    if not args.quiet:
        n_groups = len(result.fragment["groups"])
        n_commands = len(result.fragment["commands"])
        print(  # noqa: T201
            f"toolr.build: wrote {n_groups} group(s) / {n_commands} command(s) to {result.output_path}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


__all__ = [
    "BuildManifestError",
    "BuildResult",
    "build_manifest",
    "main",
]


# Re-exports for callers — discouraged direct use but accessible.
_ = (Arg, KwArg, VarArg, get_args)
