from reexport_lib import command
from toolr import command_group

command_group("grp", "Grp", "Grp")


@command(group="grp")
def ok(ctx):
    """Ok."""
