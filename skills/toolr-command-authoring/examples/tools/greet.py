"""Greeting commands.

Exercise: top-level group, two commands, the bound and the
string-attached ``@command`` forms, a ``str`` argument with a
default, a ``bool`` flag, and an ``Annotated[str, arg(...)]``
metavar override.
"""

from __future__ import annotations

from typing import Annotated

from toolr import Context
from toolr import arg
from toolr import command
from toolr import command_group

# Bound form: the returned binding's ``.command`` decorates functions
# directly. Use this when the group declaration and the commands live
# in the same file.
greet = command_group("greet", "Say hello in various ways", docstring=__doc__)


@greet.command
def hello(ctx: Context, who: str = "world", *, loud: bool = False) -> None:
    """Print a friendly greeting.

    Args:
        who: Who to greet. Defaults to ``"world"``.
        loud: Shout the greeting instead of speaking it.
    """
    msg = f"Hello, {who}!"
    if loud:
        msg = msg.upper()
    ctx.info(msg)


@command("shout", group="greet")
def shout(ctx: Context, message: Annotated[str, arg(metavar="MSG")]) -> None:
    """Shout a message in all caps.

    Args:
        message: The message to shout. Becomes ``MSG`` in ``--help``.
    """
    ctx.info(message.upper())
