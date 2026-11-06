import sys

from toolr import command_group as cg

if sys.platform:
    g = cg("g", "G", "Group.")


@g.command
def one(ctx):
    """One."""
