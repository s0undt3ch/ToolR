"""Deployment tools with mixed command structures."""

from __future__ import annotations

from toolr import Context
from toolr import registry

# Create main deployment group
deployment_group = registry.command_group("deployment", "Deployment Tools", "Application deployment and management")


# Simple commands directly on the deployment group
@deployment_group.command("status", help="Check deployment status")
def deployment_status(ctx: Context) -> None:
    """Check deployment status."""
    ctx.print("deployment status executed")


@deployment_group.command("rollback", help="Rollback deployment")
def deployment_rollback(ctx: Context) -> None:
    """Rollback deployment."""
    ctx.print("deployment rollback executed")


# Create nested command groups
k8s_group = deployment_group.command_group("k8s", "Kubernetes", "Kubernetes deployment tools")
aws_group = deployment_group.command_group("aws", "AWS", "AWS deployment tools")


# Commands in k8s group
@k8s_group.command("deploy", help="Deploy to Kubernetes")
def k8s_deploy(ctx: Context) -> None:
    """Deploy to Kubernetes."""
    ctx.print("k8s deploy executed")


@k8s_group.command("scale", help="Scale Kubernetes deployment")
def k8s_scale(ctx: Context) -> None:
    """Scale Kubernetes deployment."""
    ctx.print("k8s scale executed")


# Commands in aws group
@aws_group.command("deploy", help="Deploy to AWS")
def aws_deploy(ctx: Context) -> None:
    """Deploy to AWS."""
    ctx.print("aws deploy executed")


@aws_group.command("update", help="Update AWS deployment")
def aws_update(ctx: Context) -> None:
    """Update AWS deployment."""
    ctx.print("aws update executed")
