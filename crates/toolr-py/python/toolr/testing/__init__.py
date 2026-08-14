"""Utilities for testing toolr commands.

- `CommandsTester` — test command *discovery* (`@command_group`/`@command`
  registration) without spawning the real CLI. See `_discovery`.
- `make_context` — build a real, usable `Context` to call an
  `@command`-decorated function directly and assert on what it did. See
  `_make_context`.

The pytest plugin (`_pytest_plugin`) that makes `import tools.*` resolve
under pytest lives here too, registered via the `pytest11` entry point in
`pyproject.toml` — it isn't part of this module's public surface.
"""

from __future__ import annotations

from toolr.testing._discovery import CommandsTester
from toolr.testing._make_context import CapturedOutput
from toolr.testing._make_context import ContextForTesting
from toolr.testing._make_context import make_context
from toolr.testing._run_mock import RunMock
from toolr.testing._run_mock import make_command_result

__all__ = [
    "CapturedOutput",
    "CommandsTester",
    "ContextForTesting",
    "RunMock",
    "make_command_result",
    "make_context",
]
