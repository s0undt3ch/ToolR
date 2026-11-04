from toolr import command, command_group

NAME = "renamed"
g = command_group("grp", "Grp", "Grp")


@command(NAME, group="grp")
def positional(ctx):
    """Non-literal positional name."""


@g.command(name=NAME + "-kw")
def keyword(ctx):
    """Non-literal name keyword."""
