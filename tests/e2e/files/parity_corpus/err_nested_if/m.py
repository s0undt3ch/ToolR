import sys

from toolr import command, command_group

command_group("grp", "Grp", "Grp")

if sys.platform:

    @command(group="grp")
    def guarded(ctx):
        """Guarded."""
