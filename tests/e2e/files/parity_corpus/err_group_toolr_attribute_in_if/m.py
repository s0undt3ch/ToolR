import sys

import toolr as t

if sys.platform:
    g = t.command_group("g", "G", "Group.")


@g.command
def one(ctx):
    """One."""
