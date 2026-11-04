from toolr import command

from ._common import group


@group.command
def check_run(ctx):
    """Check the run."""


@command(group="ci")
def summary(ctx):
    """Summarise."""
