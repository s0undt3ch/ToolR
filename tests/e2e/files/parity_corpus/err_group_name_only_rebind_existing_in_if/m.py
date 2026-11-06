from toolr import command_group
ci = command_group("ci", "CI", "CI tools.")
if True:
    g = command_group("ci", "CI")
@g.command
def hello(ctx):
    """Hello."""
