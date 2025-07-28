from __future__ import annotations

from toolr import Context
from toolr import command_group

group = command_group("example", "Example Commands", "Example command group")


@group.command
def process(ctx: Context, verbose: bool = False, dry_run: bool = False):
    """Process something with optional flags.

    Args:
        verbose: Whether to print verbose output.
        dry_run: Whether to perform a dry run (no changes will be made).
    """
    if verbose:
        ctx.info("Verbose mode enabled")

    if dry_run:
        ctx.info("Dry run mode - no changes will be made")
        return

    ctx.info("Processing...")
