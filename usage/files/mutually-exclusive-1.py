from __future__ import annotations

from typing import Annotated

from toolr import Context
from toolr import arg


def process_file(
    ctx: Context,
    filename: str,
    *,
    verbose: Annotated[bool, arg(group="verbosity")] = False,
    quiet: Annotated[bool, arg(group="verbosity")] = False,
) -> None:
    """Process a file with configurable verbosity.

    Args:
        filename: The file to process.
        verbose: Enable verbose output.
        quiet: Suppress all output.
    """
    if verbose:
        ctx.info(f"Processing {filename} with verbose output...")
    elif quiet:
        # Process silently
        pass
    else:
        ctx.info(f"Processing {filename}...")
