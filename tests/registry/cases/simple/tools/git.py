"""Simple git commands for testing."""

from __future__ import annotations

from toolr import Context
from toolr import command_group

# Create a simple command group
git_group = command_group("git", "Git Commands", "Git-related tools")


@git_group.command("status", help="Show git status")
def git_status(ctx: Context, args):
    """Show git status."""
    return "git status executed"


@git_group.command("commit", help="Commit changes")
def git_commit(args):
    """Commit changes."""
    return "git commit executed"
