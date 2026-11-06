from toolr import command, command_group

command_group("grp", "Grp", "Group.")


def register():
    @command(group="grp")
    def nested(ctx):
        """Nested."""


(g := command_group("w", "W", "Walrus."))
