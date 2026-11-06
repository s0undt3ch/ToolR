from toolr import command_group as _cg


def command_group(name, title, description):
    return _cg(name, title, description)


g = command_group("w", "W", "Group.")


@g.command
def one(ctx):
    """One."""
