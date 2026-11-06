from toolr import command_group

g = command_group("grp", "Grp", "Group.")


@g.command
def one(ctx):
    """One."""
    lookup = command_group("grp")
    titled = command_group("grp", "Grp")
    ctx.print(lookup, titled)
