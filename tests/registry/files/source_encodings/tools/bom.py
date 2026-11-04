from toolr import command_group

g = command_group("grp", "Grp", "Grp")


@g.command
def bom(ctx):
    """Starts with a UTF-8 byte order mark."""
