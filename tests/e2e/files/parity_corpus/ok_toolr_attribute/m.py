import toolr
from toolr import command

toolr.command_group("grp", "Grp", "Grp")


@command(group="grp")
def plain(ctx):
    """Plain."""


@toolr.command(group="grp")
def attr(ctx):
    """Attribute."""
