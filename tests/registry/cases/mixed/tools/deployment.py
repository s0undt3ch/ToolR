"""Deployment tools with mixed command structures."""

from __future__ import annotations

from toolr import Context
from toolr import command_group

# Create main deployment group
deployment_group = command_group("deployment", "Deployment Tools", "Application deployment and management")


# Simple commands directly on the deployment group
@deployment_group.command
def status(ctx: Context) -> None:
    """Check deployment status."""
    ctx.print("deployment status executed")


@deployment_group.command("rollback")
def deployment_rollback(ctx: Context) -> None:
    """Rollback deployment."""
    ctx.print("deployment rollback executed")


# Create nested command groups
k8s_group = deployment_group.command_group("k8s", "Kubernetes", "Kubernetes deployment tools")
aws_group = deployment_group.command_group("aws", "AWS", "AWS deployment tools")


# Commands in k8s group
@k8s_group.command
def deploy(ctx: Context) -> None:
    """Deploy to Kubernetes."""
    ctx.print("k8s deploy executed")


@k8s_group.command("scale")
def k8s_scale(ctx: Context) -> None:
    """Scale Kubernetes deployment."""
    ctx.print("k8s scale executed")


# Commands in aws group
@aws_group.command("deploy")
def aws_deploy(ctx: Context) -> None:
    """Deploy to AWS."""
    ctx.print("aws deploy executed")


@aws_group.command("update")
def aws_update(ctx: Context) -> None:
    """Update AWS deployment."""
    ctx.print("aws update executed")
