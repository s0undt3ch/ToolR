from toolr import command_group

try:
    g = command_group("g", "G", "Group.")
except ImportError:
    pass


@g.command
def one(ctx):
    """One."""
