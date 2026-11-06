from tools._common import cg

g = cg("re", "Re", "Re-exported group.")


@g.command
def run(ctx):
    """Run."""
