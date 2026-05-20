"""DispatchCommand — the runtime payload injected into dispatcher commands."""

from __future__ import annotations

from typing import Any

from msgspec import Struct

from toolr.sources._types import CommandSchema  # noqa: TC001 — msgspec needs runtime annotations


def _flag_for(name: str) -> str:
    """`dry_run` → `--dry-run`."""
    return "--" + name.replace("_", "-")


class DispatchCommand(Struct, frozen=True):
    command: str
    command_args: dict[str, Any]
    schema: CommandSchema

    @property
    def argv(self) -> list[str]:
        """Argparse-shaped argv reconstructed from `command_args` per `schema`.

        For each argument in `schema.arguments` that appears in
        `command_args`, emit the appropriate token(s):

        - `positional` → bare value
        - `flag` → `--name` when truthy, omitted when falsy
        - `optional` → `--name value`, omitted when value == default
        - `repeated` → `--name value` per element

        Keys in `command_args` not found in `schema.arguments` raise
        ValueError so typos surface loudly.
        """
        known = {a.name for a in self.schema.arguments}
        for key in self.command_args:
            if key not in known:
                msg = f"DispatchCommand.argv: unknown argument {key!r} (not in schema)"
                raise ValueError(msg)

        out: list[str] = []
        for arg in self.schema.arguments:
            if arg.name not in self.command_args:
                continue
            value = self.command_args[arg.name]
            if arg.kind == "positional":
                out.append(str(value))
            elif arg.kind == "flag":
                if value:
                    out.append(_flag_for(arg.name))
            elif arg.kind == "optional":
                if arg.default is not None and str(value) == arg.default:
                    continue
                out.extend([_flag_for(arg.name), str(value)])
            elif arg.kind == "repeated":
                for element in value:
                    out.extend([_flag_for(arg.name), str(element)])
        return out
