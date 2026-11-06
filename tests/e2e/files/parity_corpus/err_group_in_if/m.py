import sys

from toolr import command_group

if sys.platform:
    g = command_group("g", "G", "Group.")


@g.command
def one(ctx):
    """One."""
