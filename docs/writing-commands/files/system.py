from __future__ import annotations

from toolr import Context
from toolr import command_group

group = command_group("system", "System Commands", "System operations")


@group.command
def info(ctx: Context) -> None:
    """Show system information.

    Displays repository root, Python version, and other system details.
    """
    # Access the repository root
    ctx.info(f"Repository root: {ctx.repo_root}")

    # Run a command
    result = ctx.run("python", "--version", capture_output=True, stream_output=False)
    ctx.info("Python version", result.stdout.read().strip())

    # Rich console output formatting available
    ctx.print("[bold green]System info retrieved successfully![/bold green]")


@group.command
def check_disk(ctx: Context, path: str = ".") -> None:
    """Check disk usage for a path.

    Args:
        path: The path to check disk usage for. Defaults to current directory.
    """
    # Run command with error handling
    try:
        result = ctx.run("du", "-sh", path, capture_output=True, stream_output=False)
        if result.returncode == 0:
            ctx.print(f"[green]Disk usage for {path}: {result.stdout.read().strip()}[/green]")
        else:
            ctx.error(f"Failed to check disk usage: {result.stderr.read().strip()}")
    except Exception as e:
        ctx.error(f"Error checking disk usage: {e}")


@group.command
def network_test(ctx: Context, host: str = "8.8.8.8", count: int = 3) -> None:
    """Test network connectivity to a host.

    Args:
        host: The host to test connectivity to. Defaults to Google's DNS (8.8.8.8).
        count: Number of ping packets to send. Defaults to 3.
    """
    ctx.info(f"Testing connectivity to {host}")

    result = ctx.run("ping", "-c", count, host, capture_output=True, stream_output=False)

    if result.returncode == 0:
        ctx.print(f"[green]Network connectivity to {host} is working[/green]")
        # Extract ping statistics
        lines = result.stdout.read().decode().split("\n")
        for line in lines:
            if "packets transmitted" in line:
                ctx.info(f"Ping statistics: {line.strip()}")
    else:
        ctx.error(f"Network connectivity to {host} failed")
