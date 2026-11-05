from toolr import command_group

g = command_group("grp", "Other", "Grp commands.")


@g.command
def two(ctx):
    """Two."""
