from collections.abc import Sequence

class CommandError(Exception):
    def __init__(self, message: str) -> None: ...

class CommandTimeoutError(CommandError):
    def __init__(self, message: str) -> None: ...

class CommandTimeoutNoOutputError(CommandError):
    def __init__(self, message: str) -> None: ...

def run_command_impl(
    args: Sequence[str],
    cwd: str | None = ...,
    env: dict[str, str] = ...,
    input: bytes | None = ...,
    stdout_fd: int | None = ...,
    stderr_fd: int | None = ...,
    sys_stdout_fd: int | None = ...,
    sys_stderr_fd: int | None = ...,
    timeout_secs: float | None = ...,
    no_output_timeout_secs: float | None = ...,
) -> int: ...
