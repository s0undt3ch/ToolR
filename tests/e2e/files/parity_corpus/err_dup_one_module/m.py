from toolr import command_group

g = command_group("grp", "Grp", "Grp")


@g.command
def dup(ctx):
    """One."""


@g.command(name="dup")
def dup2(ctx):
    """Two."""
