from toolr import command_group

g = command_group("grp", "Grp", "Grp")


@g.command
def dup(ctx):
    """Dup."""
