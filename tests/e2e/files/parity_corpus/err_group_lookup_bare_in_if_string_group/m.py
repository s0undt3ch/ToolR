from toolr import command, command_group
if True:
    command_group("new", "New")
@command(group="new")
def hello(ctx):
    """Hello."""
