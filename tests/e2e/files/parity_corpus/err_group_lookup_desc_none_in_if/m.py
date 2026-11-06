from toolr import command_group
if True:
    g = command_group("new", "New", None)
@g.command
def hello(ctx):
    """Hello."""
