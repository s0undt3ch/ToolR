from toolr import command_group

OPTIONS = {"description": "CI commands."}

g = command_group("ci", "CI", **OPTIONS)


@g.command
def one(ctx):
    """One."""
