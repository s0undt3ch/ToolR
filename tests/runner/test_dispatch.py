from __future__ import annotations

import os
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

import pytest


def test_runner_invokes_target_function(
    tools_module: Callable[[str], Path],
    spec_file: Callable[..., Path],
    run_runner: Callable[[Path], subprocess.CompletedProcess[str]],
) -> None:
    tools_module(
        """
        from toolr import command_group

        group = command_group("demo", "Demo", description="demo group")

        @group.command
        def hello(ctx, name: str = "world") -> None:
            "Do it."
            ctx.print(f"hi {name}")
        """
    )
    spec_path = spec_file(command="hello", function="hello", args={"name": "Alice"})
    result = run_runner(spec_path)
    assert result.returncode == 0, f"stderr:\n{result.stderr}\nstdout:\n{result.stdout}"
    assert "hi Alice" in result.stdout


def test_runner_invokes_target_with_positional_before_variadic(
    tools_module: Callable[[str], Path],
    spec_file: Callable[..., Path],
    run_runner: Callable[[Path], subprocess.CompletedProcess[str]],
) -> None:
    tools_module(
        """
        import enum
        import pathlib

        from toolr import Context
        from toolr import command_group

        group = command_group("demo", "Demo", description="demo group")

        class Foo(enum.Enum):
            A = "a"
            B = "b"

        @group.command
        def positional(ctx: Context, foo: Foo, *paths: pathlib.Path) -> None:
            "Do it."
            ctx.print(f"foo: {foo}")
            ctx.print(f"paths: {[p.as_posix() for p in paths]}")
        """
    )
    spec_path = spec_file(
        command="positional",
        function="positional",
        args={"foo": "a", "paths": ["tools/env/setup.py.bak", "tools/example.py"]},
    )
    result = run_runner(spec_path)
    assert result.returncode == 0, f"stderr:\n{result.stderr}\nstdout:\n{result.stdout}"
    assert "foo: Foo.A" in result.stdout
    assert "paths: ['tools/env/setup.py.bak', 'tools/example.py']" in result.stdout


def test_runner_propagates_nonzero_exit_via_ctx_exit(
    tools_module: Callable[[str], Path],
    spec_file: Callable[..., Path],
    run_runner: Callable[[Path], subprocess.CompletedProcess[str]],
) -> None:
    tools_module(
        """
        from toolr import command_group

        group = command_group("demo", "Demo", description="demo group")

        @group.command
        def boom(ctx) -> None:
            "Do it."
            ctx.exit(7, "failing on purpose")
        """
    )
    spec_path = spec_file(command="boom", function="boom")
    result = run_runner(spec_path)
    assert result.returncode == 7


@pytest.mark.skip_on_windows(reason="POSIX signals")
def test_runner_exits_128_plus_signal_for_signal_killed_ctx_run(
    tools_module: Callable[[str], Path],
    spec_file: Callable[..., Path],
    run_runner: Callable[[Path], subprocess.CompletedProcess[str]],
) -> None:
    tools_module(
        """
        from toolr import command_group

        group = command_group("demo", "Demo", description="demo group")

        @group.command
        def boom(ctx) -> None:
            "Do it."
            result = ctx.run("sh", "-c", "kill -TERM $$")
            ctx.exit(result.returncode, "child was killed")
        """
    )
    spec_path = spec_file(command="boom", function="boom")
    result = run_runner(spec_path)
    assert result.returncode == 143


def test_runner_propagates_exception_as_exit_1(
    tools_module: Callable[[str], Path],
    spec_file: Callable[..., Path],
    run_runner: Callable[[Path], subprocess.CompletedProcess[str]],
) -> None:
    tools_module(
        """
        from toolr import command_group

        group = command_group("demo", "Demo", description="demo group")

        @group.command
        def crash(ctx) -> None:
            "Do it."
            raise RuntimeError("crashed")
        """
    )
    spec_path = spec_file(command="crash", function="crash")
    result = run_runner(spec_path)
    assert result.returncode == 1
    assert "RuntimeError" in result.stderr
    assert "crashed" in result.stderr


def test_runner_fails_clearly_when_spec_env_unset(tmp_path: Path) -> None:
    # This test exercises the env-unset path directly without the factory
    # fixtures because the whole point is the absence of ``TOOLR_SPEC_FILE``.
    env = os.environ.copy()
    env.pop("TOOLR_SPEC_FILE", None)
    result = subprocess.run(
        [sys.executable, "-m", "toolr._runner"],
        env=env,
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode != 0
    assert "TOOLR_SPEC_FILE" in result.stderr
