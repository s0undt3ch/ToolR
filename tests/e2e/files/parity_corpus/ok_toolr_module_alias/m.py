import toolr as t
from toolr import command

t.command_group("grp", "Grp", "Grp")


@command(group="grp")
def plain(ctx):
    """Plain."""


@t.command(name="aliased", group="grp")
def attr(ctx):
    """Alias."""
