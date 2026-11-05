import toolr as t
from toolr import command_group

command_group("grp", "Grp", "Grp")
command = t.command
cmd = command


@command(group="grp")
def ok(ctx):
    """Ok."""


@cmd(group="grp")
def also(ctx):
    """Also."""
