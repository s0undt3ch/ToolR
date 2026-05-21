"""Schema types for externally-discovered toolr commands.

`ArgSchema` and `CommandSchema` are produced by the Rust argparse
scanner (or, in the future, by external source plugins) and shipped
through the manifest. They are also exposed on
`DispatchCommand.schema` so dispatcher bodies can reconstruct argv.
"""

from __future__ import annotations

from typing import Literal

from msgspec import Struct


class ArgSchema(Struct, frozen=True):
    """One argument on a discovered command.

    Mirrors the argparse `add_argument` fields the scanner can extract.
    Anything the scanner can't statically resolve is left at its default
    (`None`) and recorded as a warning at scan time.
    """

    name: str
    kind: Literal["positional", "optional", "flag", "repeated"]
    help: str = ""
    default: str | None = None
    choices: list[str] | None = None
    metavar: str | None = None
    type_annotation: str | None = None  # "str" / "int" / "float" / "bool"
    nargs: Literal["*", "+", "?"] | int | None = None
    long_flag: str | None = None
    """Literal long-flag spelling from the source (e.g. `"--user_ids"`).

    Populated by the argparse scanner for keyword-style args; `None`
    for positionals. `DispatchCommand.argv` uses this verbatim so the
    upstream tool's exact spelling is preserved across the round-trip,
    even when toolr's CLI normalises display to `--user-ids`.
    """


class CommandSchema(Struct, frozen=True):
    """One command discovered by the argparse scanner.

    `arguments` carries only the command-specific args. Hoisted
    common_args (declared in `[tool.toolr.argparse.<name>]`) are
    applied at attach time and merged with `arguments`; consumers see
    a single combined list on the manifest side.
    """

    name: str
    summary: str
    description: str
    arguments: list[ArgSchema]
