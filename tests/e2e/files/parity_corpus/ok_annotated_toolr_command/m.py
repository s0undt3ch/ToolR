from toolr import command
from toolr import command_group

command_group("grp", "Grp", "Grp")
cmd: object = command


@cmd(group="grp")
def ok(ctx):
    """Ok."""
