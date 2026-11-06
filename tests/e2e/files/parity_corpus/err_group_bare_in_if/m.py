import sys

from toolr import command, command_group

if sys.platform:
    command_group("g", "G", "Group.")


@command(group="g")
def one(ctx):
    """One."""
