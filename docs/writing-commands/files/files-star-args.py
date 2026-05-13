from __future__ import annotations

from toolr import Context
from toolr import command
from toolr import command_group

command_group("files", "File Commands", "File operations")


@command(group="files")
def process_files(ctx: Context, *files: str):
    """Process multiple files.

    Args:
        files: The files to process.
    """
    for file in files:
        ctx.info(f"Processing {file}...")
