from toolr import command_group

g = command_group("grp", "Grp")


@g.command
def two(ctx):
    """Two."""
