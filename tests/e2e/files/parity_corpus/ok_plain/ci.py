from toolr import command, command_group

group = command_group("ci", "CI", "CI utilities")
command_group("ci.empty", "Empty", "A group with no commands")


@group.command
def hello(ctx):
    """Say hello."""


@command("check-run", group="ci")
def check(ctx):
    """Check the run."""
