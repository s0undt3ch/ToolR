"""The manifest build's unknown-group error, ported for ``CommandsTester``.

Each helper names the Rust function it mirrors; change both together so
``CommandsTester`` and ``toolr`` keep giving the same message.
"""

from __future__ import annotations

from collections.abc import Callable
from collections.abc import Iterable
from collections.abc import Sequence
from pathlib import Path
from typing import Any

from toolr._decorators import PendingCommand
from toolr._decorators import _group_path


def _is_control(char: str) -> bool:
    """C0, DEL and C1 controls. Mirrors ``crates/toolr-core/src/text.rs::is_control``."""
    code = ord(char)
    return code <= 0x1F or 0x7F <= code <= 0x9F


def _is_invisible_format(char: str) -> bool:
    """Zero-width and bidi format characters.

    Mirrors ``crates/toolr-core/src/text.rs::is_invisible_format``.
    """
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
    r"""Show control and invisible format characters as ``\u{..}``.

    Mirrors ``crates/toolr-core/src/text.rs::escape_control_chars``.
    """
    return "".join(
        f"\\u{{{ord(char):x}}}" if _is_control(char) or _is_invisible_format(char) else char
        for char in text
    )


def _edit_distance(a: str, b: str) -> int:
    """Levenshtein distance over characters.

    Mirrors ``crates/toolr-core/src/parser/build.rs::edit_distance``, not the
    byte-counting one in ``argparse/attach.rs``.
    """
    previous = list(range(len(b) + 1))
    for i, char_a in enumerate(a, start=1):
        current = [i]
        for j, char_b in enumerate(b, start=1):
            cost = 0 if char_a == char_b else 1
            current.append(min(previous[j] + 1, current[j - 1] + 1, previous[j - 1] + cost))
        previous = current
    return previous[-1]


def _nearest_group(target: str, registered: list[str]) -> str | None:
    """The closest declared group to ``target`` within the threshold, if any.

    Mirrors ``crates/toolr-core/src/parser/build.rs::nearest_group``. The
    threshold uses the UTF-8 length, as Rust's ``str::len`` does.

    Ties differ from Rust: this takes the first of equally close groups in
    ``registered`` (sorted), while Rust takes whichever its ``HashSet``
    yields first. Parity fixtures must avoid ties.
    """
    limit = max(len(target.encode()) // 3, 2)
    best: tuple[int, str] | None = None
    for candidate in registered:
        distance = _edit_distance(target, candidate)
        if distance <= limit and (best is None or distance < best[0]):
            best = (distance, candidate)
    return None if best is None else best[1]


def _unknown_group_line(group: str, module: str, command: str, registered: list[str]) -> str:
    """One line of the error.

    Mirrors the loop body of ``crates/toolr-core/src/parser/build.rs::format_unknown_groups``.
    """
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
    """Sort key for a command's file, matching the manifest build's walk order.

    Mirrors the path sort in ``crates/toolr-core/src/hash.rs::list_python_files``:
    files under ``tools_dir`` sort by path components. Functions defined
    elsewhere (a test body, say) sort after them; the stable sort keeps
    their order.
    """
    try:
        relative = Path(func.__code__.co_filename).relative_to(tools_dir)
    except ValueError:
        return (1, ())
    return (0, relative.parts)


def _format_unknown_groups(
    pending: Sequence[PendingCommand], registered: Iterable[str], tools_dir: Path
) -> str:
    """The whole error for ``pending``, given the declared groups' full names.

    Mirrors ``crates/toolr-core/src/parser/build.rs::format_unknown_groups``
    and the ``BuildError::UnknownGroupRefs`` header.
    """
    paths = sorted(_group_path(full_name) for full_name in registered)
    ordered = sorted(pending, key=lambda entry: _source_order_key(entry.func, tools_dir))
    lines = [
        _unknown_group_line(entry.group, entry.func.__module__, entry.name, paths)
        for entry in ordered
    ]
    return "\n".join([f"unknown group references ({len(lines)}):", *lines])
