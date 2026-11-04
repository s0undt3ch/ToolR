from toolr import command, command_group
from toolr import command as cmd

command_group("grp", "Grp", "Grp")


@command(group="grp")
def plain(ctx):
    """Plain."""


@cmd(group="grp")
def aliased(ctx):
    """Aliased."""
