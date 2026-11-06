from toolr import command_group
if True:
    g = command_group("new", "New")
@g.command
def hello(ctx):
    """Hello."""
