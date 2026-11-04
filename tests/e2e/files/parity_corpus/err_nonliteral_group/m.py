from toolr import command, command_group

GROUP = "grp"
command_group(GROUP, "Grp", "Grp")


@command(group=GROUP)
def nonlit(ctx):
    """Non-literal group."""
