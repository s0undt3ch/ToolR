from toolr import command_group

g = command_group("grp", "Grp", description="")


@g.command
def one(ctx):
    """One."""
