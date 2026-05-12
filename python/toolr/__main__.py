"""`python -m toolr` deprecation shim.

The Python-based CLI has been replaced by a Rust binary. This module
keeps `python -m toolr <args>` working by locating the `toolr` binary
and exec'ing it with the original `sys.argv`.

A one-time deprecation note is written to stderr unless the environment
variable ``TOOLR_NO_DEPRECATION_NOTICE`` is set to a truthy value.
"""

from __future__ import annotations

import os
import shutil
import sys
import sysconfig
import warnings
from pathlib import Path
from typing import NoReturn

_BINARY_NAME = "toolr.exe" if os.name == "nt" else "toolr"
_NOTICE_SHOWN_ENV = "_TOOLR_SHIM_NOTICE_SHOWN"
_SUPPRESS_NOTICE_ENV = "TOOLR_NO_DEPRECATION_NOTICE"
_DISABLE_INTERPRETER_BIN_ENV = "TOOLR_SHIM_DISABLE_INTERPRETER_BIN"


def _truthy(value: str | None) -> bool:
    return (value or "").strip().lower() in {"1", "true", "yes", "on"}


def _interpreter_bin_dir() -> Path | None:
    """Best-effort lookup for the bin dir of the active interpreter."""
    scripts = sysconfig.get_path("scripts")
    if scripts:
        return Path(scripts)
    return None


def _candidate_paths() -> list[Path]:
    """Order: PATH (via shutil.which) -> interpreter bin dir -> none."""
    candidates: list[Path] = []
    on_path = shutil.which(_BINARY_NAME)
    if on_path is not None:
        candidates.append(Path(on_path))
    if not _truthy(os.environ.get(_DISABLE_INTERPRETER_BIN_ENV)):
        scripts = _interpreter_bin_dir()
        if scripts is not None:
            binary = scripts / _BINARY_NAME
            if binary.is_file():
                candidates.append(binary)
    return candidates


def _is_self(binary: Path) -> bool:
    """Guard against accidentally exec'ing ourselves on bizarre PATH setups."""
    try:
        return binary.resolve() == Path(sys.argv[0]).resolve()
    except OSError:
        return False


def _emit_deprecation_notice() -> None:
    if _truthy(os.environ.get(_SUPPRESS_NOTICE_ENV)):
        return
    if _truthy(os.environ.get(_NOTICE_SHOWN_ENV)):
        return
    os.environ[_NOTICE_SHOWN_ENV] = "1"
    warnings.warn(
        "`python -m toolr` is deprecated; invoke the `toolr` binary directly. "
        "Set TOOLR_NO_DEPRECATION_NOTICE=1 to silence this notice.",
        DeprecationWarning,
        stacklevel=2,
    )


def main(argv: list[str] | None = None) -> NoReturn:
    """Locate the `toolr` binary and exec it with the given argv."""
    _emit_deprecation_notice()
    args = list(sys.argv[1:] if argv is None else argv)
    for candidate in _candidate_paths():
        if _is_self(candidate):
            continue
        try:
            os.execv(str(candidate), [str(candidate), *args])
        except OSError as exc:
            print(
                f"toolr: failed to exec {candidate}: {exc}",
                file=sys.stderr,
            )
            continue
    print(
        "toolr: `toolr` binary not found on PATH or alongside the current "
        "Python interpreter. Install it via the install.sh script, mise, "
        "GitHub releases, or pip install (which currently does not bundle "
        "the binary — see CHANGELOG).",
        file=sys.stderr,
    )
    sys.exit(127)


if __name__ == "__main__":
    main()
