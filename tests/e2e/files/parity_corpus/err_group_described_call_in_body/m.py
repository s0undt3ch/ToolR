from toolr import command_group

g = command_group("grp", "Grp", "Group.")


@g.command
def one(ctx):
    """One."""
    command_group("grp", "Grp", "Group.")
