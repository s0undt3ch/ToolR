from toolr import command
from toolr import command_group

command_group("grp", "Grp", "Grp")
command: object


@command(group="grp")
def ok(ctx):
    """Ok."""
