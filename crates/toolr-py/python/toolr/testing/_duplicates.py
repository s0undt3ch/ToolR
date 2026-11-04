"""The manifest build's duplicate-commands error, ported for ``CommandsTester``.

Each helper names the Rust function it mirrors; change both together so
``CommandsTester`` and ``toolr`` keep giving the same message.
"""

from __future__ import annotations

from collections.abc import Callable
from collections.abc import Sequence
from pathlib import Path
from typing import Any

from toolr._decorators import DuplicateCommand
from toolr._decorators import _group_path
from toolr.testing._unknown_groups import _escape_control_chars
from toolr.testing._unknown_groups import _source_order_key


def _build_order_key(func: Callable[..., Any], tools_dir: Path) -> tuple[Any, ...]:
    """Sorted file order, then source order, as the manifest build walks them."""
    return (*_source_order_key(func, tools_dir), func.__code__.co_firstlineno)


def _command_path(group: str, name: str) -> str:
    """Mirrors ``crates/toolr-core/src/third_party/merge.rs::command_path``."""
    return f"{group.replace('.', ' ')} {name}" if group else name


def _format_duplicate_commands(duplicates: Sequence[DuplicateCommand], tools_dir: Path) -> str:
    """The whole error for ``duplicates``.

    Mirrors ``crates/toolr-core/src/parser/build.rs::format_duplicate_commands``
    and the ``BuildError::DuplicateCommands`` header.
    """
    definitions: dict[tuple[str, str], list[Callable[..., Any]]] = {}
    for entry in duplicates:
        funcs = definitions.setdefault((_group_path(entry.group), entry.name), [])
        funcs.extend(func for func in (entry.first, entry.second) if func not in funcs)
    for funcs in definitions.values():
        funcs.sort(key=lambda func: _build_order_key(func, tools_dir))
    keys = sorted(definitions, key=lambda key: _build_order_key(definitions[key][0], tools_dir))
    lines = []
    for group, name in keys:
        places = ", ".join(
            f"{_escape_control_chars(func.__module__)}::{_escape_control_chars(func.__name__)}"
            for func in definitions[(group, name)]
        )
        lines.append(
            f"  - `{_escape_control_chars(_command_path(group, name))}` is defined more than "
            f'once: {places}. Rename all but one, e.g. with `name="…"`.'
        )
    return "\n".join([f"duplicate commands ({len(lines)}):", *lines])
