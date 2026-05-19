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
