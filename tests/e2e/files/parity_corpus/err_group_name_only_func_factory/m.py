from toolr import command_group
ci = command_group("ci", "CI", "CI tools.")
def get():
    return command_group("ci")
g = get()
@g.command
def hello(ctx):
    """Hello."""
