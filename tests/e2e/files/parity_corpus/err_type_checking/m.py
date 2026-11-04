from typing import TYPE_CHECKING

from toolr import command, command_group

command_group("grp", "Grp", "Grp")


@command(group="grp")
def ok(ctx):
    """Ok."""


if TYPE_CHECKING:

    @command(group="grp")
    def hidden(ctx):
        """Hidden."""
