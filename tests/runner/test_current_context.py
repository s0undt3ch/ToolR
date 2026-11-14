"""The runner sets ``toolr._context._current_ctx`` before dispatching.

The subprocess tests here spawn the real ``python -m toolr._runner`` (same
pattern as ``tests/runner/test_dispatch.py``); the in-process test calls
``run()`` directly so the wiring itself is measured by coverage.
"""

from __future__ import annotations

import contextvars
import subprocess
from collections.abc import Callable
from pathlib import Path

import msgspec

from toolr._context import _current_ctx
from toolr._runner import RunnerSpec
from toolr._runner import run


def test_current_context_is_set_before_target_runs(
    tools_module: Callable[[str], Path],
    spec_file: Callable[..., Path],
    run_runner: Callable[[Path], subprocess.CompletedProcess[str]],
) -> None:
    tools_module(
        """
        from toolr import command_group
        from toolr._context import current_context

        group = command_group("demo", "Demo", description="demo group")

        @group.command
        def hello(ctx, name: str = "world") -> None:
            "Do it."
            same = current_context() is ctx
            ctx.print(f"hi {name} same={same}")
        """
    )
    spec_path = spec_file(command="hello", function="hello", args={"name": "Alice"})
    result = run_runner(spec_path)
    assert result.returncode == 0, f"stderr:\n{result.stderr}\nstdout:\n{result.stdout}"
    assert "hi Alice same=True" in result.stdout


def test_current_context_works_at_module_import_time(
    tools_module: Callable[[str], Path],
    spec_file: Callable[..., Path],
    run_runner: Callable[[Path], subprocess.CompletedProcess[str]],
) -> None:
    """The runner sets the contextvar *before* importing the target module, so a
    module-level call succeeds — it must never regress to failing here.
    """
    tools_module(
        """
        from toolr import command_group
        from toolr._context import current_context

        # Module-level call: this runs during `_import_target()`.
        _repo_root_at_import = current_context().repo_root

        group = command_group("demo", "Demo", description="demo group")

        @group.command
        def hello(ctx) -> None:
            "Do it."
            ctx.print(f"import-time repo_root matched={_repo_root_at_import == ctx.repo_root}")
        """
    )
    spec_path = spec_file(command="hello", function="hello")
    result = run_runner(spec_path)
    assert result.returncode == 0, f"stderr:\n{result.stderr}\nstdout:\n{result.stdout}"
    assert "import-time repo_root matched=True" in result.stdout


def test_run_sets_the_contextvar_to_the_context_it_builds(
    tools_module: Callable[[str], Path],
    spec_payload: Callable[..., dict[str, object]],
) -> None:
    """In-process counterpart: ``run()`` itself sets the var to the ``ctx`` it passes on."""
    tools_module(
        """
        from toolr._context import current_context

        SEEN = {}

        def record(ctx) -> None:
            SEEN["same"] = current_context() is ctx
        """
    )
    spec = msgspec.convert(spec_payload(command="record", function="record"), type=RunnerSpec)
    # copy_context() keeps the runner's `.set()` out of the test session's
    # own context, so later tests still see an unset var.
    rc = contextvars.copy_context().run(run, spec)
    assert rc == 0
    # Deferred import is intentional: the `tools` package is created at
    # runtime and only becomes importable after `run()` appends repo_root.
    import tools.demo  # noqa: PLC0415 — created at runtime

    assert tools.demo.SEEN == {"same": True}
    assert _current_ctx.get(None) is None
