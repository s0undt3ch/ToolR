"""Dynamic manifest introspection helper.

Invoked as ``python -m toolr._introspect`` from the Rust side inside the
project's tools venv. Walks the ``command_group`` registry after
importing every module under ``tools.*``, enumerates ``importlib.metadata``
entry points in the ``toolr.commands`` group, and writes a JSON payload
to stdout.

The wire format is defined in ``specs/rust-front-end/07-plan-6-dynamic-manifest.md``.
Bump ``PAYLOAD_SCHEMA_VERSION`` on every breaking change.
"""

from __future__ import annotations

import argparse
import json
import sys
from typing import Any

PAYLOAD_SCHEMA_VERSION = 1


def build_payload(tools_root: str | None) -> dict[str, Any]:
    """Construct the dynamic-layer payload for the current Python env.

    Args:
        tools_root: Absolute path to the project's ``tools/`` directory,
            or ``None`` if the caller could not resolve one. When given,
            the helper inserts the parent of ``tools_root`` on
            ``sys.path`` so ``import tools.<sub>`` works.
    """
    warnings: list[str] = []
    groups: list[dict[str, Any]] = []
    commands: list[dict[str, Any]] = []

    # Tasks 3 and 4 fill these in; for now we emit an empty payload so
    # the wiring works end-to-end.
    _ = tools_root

    return {
        "payload_schema_version": PAYLOAD_SCHEMA_VERSION,
        "groups": groups,
        "commands": commands,
        "warnings": warnings,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="toolr._introspect",
        description="Dump toolr dynamic-layer manifest as JSON to stdout.",
    )
    parser.add_argument(
        "--tools-root",
        default=None,
        help="Absolute path to the project's tools/ directory.",
    )
    args = parser.parse_args(argv)
    payload = build_payload(args.tools_root)
    json.dump(payload, sys.stdout, separators=(",", ":"))
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
