from typing import Annotated

from toolr import command_group

g: Annotated[object, command_group("a", "A", "Group.")] = command_group("a", "A", "Group.")


@g.command
def one(ctx):
    """One."""
