from toolr import command_group

g = command_group("grp", "Grp", "Grp commands.")


@g.command
def one(ctx):
    """One."""
