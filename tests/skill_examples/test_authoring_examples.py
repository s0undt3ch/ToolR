"""Example tests extracted verbatim into `skills/toolr-command-authoring`'s
testing reference by `cargo xtask build-skill-refs`.

Keep each example function self-contained and dedented — its source becomes
a fenced code block as-is. Renaming a function referenced by the generator
fails `cargo xtask build-skill-refs --check` until the skill reference is
regenerated to match.
"""

from __future__ import annotations

import pytest

from toolr import Context
from toolr.testing import RunMock
from toolr.testing import make_command_result
from toolr.testing import make_context


def deploy(ctx: Context) -> None:
    result = ctx.run("git", "status", "--porcelain", capture_output=True)
    if result.stdout.read().strip():
        ctx.error("working tree is dirty — commit or stash before deploying")
        ctx.exit(1)


def test_deploy_checks_git_status(tmp_path):
    run = RunMock()
    run.mock.return_value = make_command_result(stdout="")
    ctx = make_context(repo_root=tmp_path, run=run)

    deploy(ctx)

    run.assert_called_once_with(
        ("git", "status", "--porcelain"),
        stream_output=True,
        capture_output=True,
        timeout_secs=None,
        no_output_timeout_secs=None,
    )


def confirm_deploy(ctx: Context) -> None:
    if not ctx.prompt("Deploy to production?", bool, default=False):
        ctx.error("Aborted.")
        ctx.exit(1)
    ctx.print("Deploying...")


def test_confirm_deploy_aborts_without_confirmation(tmp_path):
    ctx = make_context(repo_root=tmp_path, prompt_input="n\n")

    with pytest.raises(SystemExit):
        confirm_deploy(ctx)

    assert "Aborted." in ctx.stderr
