""""""
from toolr import command_group

g = command_group("ci", "CI", description="CI commands.", docstring=__doc__)


@g.command
def one(ctx):
    """One."""
