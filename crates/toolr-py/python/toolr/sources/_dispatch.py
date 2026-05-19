"""DispatchCommand — the runtime payload injected into dispatcher commands.

A dispatcher command is a user-written toolr command whose signature
declares exactly one keyword-only parameter annotated as
DispatchCommand. When the runtime matches one of the dispatcher's
attached children, it constructs this object and passes it in as the
value of that parameter.

`argv` reconstructs argparse-shaped argv — typically used by the
dispatcher body to forward to a subprocess (e.g.
`ctx.run('python', 'manage.py', *dispatched.argv)`).
"""

from __future__ import annotations

from typing import Any

from msgspec import Struct

from toolr.sources._types import CommandSchema  # noqa: TC001


class DispatchCommand(Struct, frozen=True):
    command: str
    command_args: dict[str, Any]
    schema: CommandSchema
