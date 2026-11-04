from nested_lib import command
from toolr import command_group

command_group("grp", "Grp", "Grp")


@command(group="grp")
def ok(ctx):
    """Ok."""


def register():
    @command(group="grp")
    def nested(ctx):
        """Nested."""
