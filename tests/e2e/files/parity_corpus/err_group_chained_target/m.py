from toolr import command_group

a = b = command_group("g", "G", "Group.")


@a.command
def one(ctx):
    """One."""
