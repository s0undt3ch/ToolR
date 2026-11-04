import sys

from toolr import command, command_group

command_group("grp", "Grp", "Grp")


@command(group="grp")
def ok(ctx):
    """Ok."""


if sys.platform == "no-such-platform":

    @command(group="grp")
    def never(ctx):
        """Never."""
