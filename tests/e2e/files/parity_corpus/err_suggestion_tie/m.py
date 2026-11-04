from toolr import command, command_group

command_group("ab", "AB", "AB")
command_group("ca", "CA", "CA")
command_group("ac", "AC", "AC")


@command(group="aa")
def typo(ctx):
    """Typo."""
