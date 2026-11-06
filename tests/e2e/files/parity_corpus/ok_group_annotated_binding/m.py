from toolr import command_group
from toolr._decorators import CommandGroup

h: CommandGroup = command_group("ann", "Ann", "Annotated group.")


@h.command
def run(ctx):
    """Run."""
