"""Simple git commands for testing."""

from __future__ import annotations

from toolr import Context
from toolr import command_group

# Create a simple command group
git_group = command_group("git", "Git Commands", "Git-related tools")


@git_group.command("status")
def git_status(ctx: Context) -> None:
    """Show git status."""
    ctx.print("git status executed")


@git_group.command("commit")
def git_commit(ctx: Context) -> None:
    """Commit changes."""
    ctx.print("git commit executed")
