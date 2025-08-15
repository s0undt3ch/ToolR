"""
Public interface to the rust command extension module.

It provides a simple interface to run commands and stream/capture their output.
"""

from __future__ import annotations

import io
import os
import pathlib
import sys
import tempfile
from collections.abc import Sequence
from typing import IO
from typing import TYPE_CHECKING
from typing import Any
from typing import Generic
from typing import TypeAlias
from typing import TypeVar
from typing import cast

from msgspec import Struct

from ._command import CommandError  # noqa: F401
from ._command import CommandTimeoutError  # noqa: F401
from ._command import CommandTimeoutNoOutputError  # noqa: F401
from ._command import run_command_impl

# Define our type variables
T = TypeVar("T", str, bytes)
ENVIRON: TypeAlias = dict[str, str] | None


class CommandResult(Struct, Generic[T], frozen=True):
    """
    The result of a command execution.
    """

    args: list[str]
    stdout: IO[T]
    stderr: IO[T]
    returncode: int


def run(  # noqa: PLR0915
    args: Sequence[str],
    *,
    cwd: str | pathlib.Path | None = None,
    env: ENVIRON = None,
    input: str | bytes | None = None,  # noqa: A002
    stream_output: bool = False,
    capture_output: bool = False,
    text: bool = True,
    encoding: str | None = "utf-8",
    timeout_secs: float | None = None,
    no_output_timeout_secs: float | None = None,
) -> CommandResult[str] | CommandResult[bytes]:
    """
    Run a command in a subprocess.

    Args:
        args: Command and arguments to run
        cwd: Current working directory to run the command in. Defaults to the current directory.
        env: Environment variables to pass to the command
        input: Input data to pass to the command
        stream_output: Whether to stream output to stdout/stderr
        capture_output: Whether to capture output to return
        text: Whether to return output as text or bytes
        encoding: Encoding to use for text output
        timeout_secs: Maximum time to wait for command completion
        no_output_timeout_secs: Maximum time to wait without output

    Returns:
        CommandResult object containing stdout, stderr, and return code

    Raises:
        CommandError: If any of the pre-run checks fail or an operational failure happens.
        CommandTimeoutError: If the command times out
        CommandTimeoutNoOutputError: If the command produces no output for too long
    """
    # Stream output is only supported with text=True
    if stream_output and not text:
        err_msg = "stream_output=True requires text=True"
        raise ValueError(err_msg)

    if cwd is None:
        cwd = pathlib.Path.cwd()

    # Initialize file variables with explicit types
    stdout_file: IO[Any] | None = None
    stderr_file: IO[Any] | None = None
    stdout_fd: int | None = None
    stderr_fd: int | None = None
    sys_stdout_fd: int | None = None
    sys_stderr_fd: int | None = None

    try:
        # Process the input data
        input_bytes = None
        if input is not None:
            if isinstance(input, str):
                input_bytes = input.encode(encoding or "utf-8")
            else:
                input_bytes = input

        # Prepare environment
        env_dict: dict[str, str] = {}
        if env:
            env_dict.update(env)
        else:
            # If no environment provided, inherit the current environment
            env_dict.update(os.environ)

        # Set up stdout/stderr handling
        if capture_output:
            if text:
                stdout_file = tempfile.TemporaryFile(mode="w+", encoding=encoding)  # noqa: SIM115
                stderr_file = tempfile.TemporaryFile(mode="w+", encoding=encoding)  # noqa: SIM115
            else:
                stdout_file = tempfile.TemporaryFile(mode="wb+")  # noqa: SIM115
                stderr_file = tempfile.TemporaryFile(mode="wb+")  # noqa: SIM115

            stdout_fd = stdout_file.fileno()
            stderr_fd = stderr_file.fileno()

        # Get sys.stdout and sys.stderr file descriptors if streaming
        if stream_output:
            try:
                sys_stdout_fd = sys.stdout.fileno()
                sys_stderr_fd = sys.stderr.fileno()
            except io.UnsupportedOperation:
                if TYPE_CHECKING:
                    assert sys.__stdout__ is not None
                    assert sys.__stderr__ is not None

                sys_stdout_fd = sys.__stdout__.fileno()
                sys_stderr_fd = sys.__stderr__.fileno()

        # Run the command implementation
        command_args = list(args)
        returncode = run_command_impl(
            command_args,
            cwd=str(cwd),
            env=env_dict,
            input=input_bytes,
            stdout_fd=stdout_fd,
            stderr_fd=stderr_fd,
            sys_stdout_fd=sys_stdout_fd,
            sys_stderr_fd=sys_stderr_fd,
            timeout_secs=timeout_secs,
            no_output_timeout_secs=no_output_timeout_secs,
        )

        if TYPE_CHECKING:
            assert stdout_file is not None
            assert stderr_file is not None

        # Rewind files for reading
        if stdout_file:
            stdout_file.seek(0)
        if stderr_file:
            stderr_file.seek(0)

        # Return the result with correct typing
        if text is True:
            return cast(
                "CommandResult[str]",
                CommandResult(args=command_args, stdout=stdout_file, stderr=stderr_file, returncode=returncode),
            )
        return cast(
            "CommandResult[bytes]",
            CommandResult(args=command_args, stdout=stdout_file, stderr=stderr_file, returncode=returncode),
        )

    except Exception as exc:
        # Clean up on error
        if stdout_file and hasattr(stdout_file, "close"):
            stdout_file.close()
        if stderr_file and hasattr(stderr_file, "close"):
            stderr_file.close()

        # Re-raise the exception
        raise exc from None
