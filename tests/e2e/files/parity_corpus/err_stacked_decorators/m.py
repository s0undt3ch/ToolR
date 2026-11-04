import toolr

g1 = toolr.command_group("g1", "G", "G")
g2 = toolr.command_group("g2", "G", "G")


@g1.command
@g2.command
def both(ctx):
    """Both."""


@g1.command
@toolr.command(group="g2")
def mixed(ctx):
    """Mixed."""
