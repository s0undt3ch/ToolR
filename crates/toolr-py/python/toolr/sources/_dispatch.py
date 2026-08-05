"""DispatchCommand — the runtime payload injected into dispatcher commands."""

from __future__ import annotations

from typing import Any

from msgspec import Struct

from toolr.sources._types import ArgSchema  # noqa: TC001 — msgspec needs runtime annotations
from toolr.sources._types import CommandSchema  # noqa: TC001 — msgspec needs runtime annotations


def _flag_for(name: str) -> str:
    """Fallback flag formatter for args with no recorded literal.

    Native toolr commands don't dispatch via this code path, so callers
    that hit this branch always have a discovered-source arg whose
    long_flag should have been populated. Treat this as a defensive
    last resort: hyphenate the param name into a CLI-friendly form so
    the result is at least usable.
    """
    return "--" + name.replace("_", "-")


def _flag_for_arg(arg: ArgSchema) -> str:
    """Return the literal long flag for `arg`, preferring the source-recorded form.

    Falls back to `_flag_for(arg.name)` when `long_flag` is absent —
    older manifests written before the field existed, native toolr
    commands, or future non-argparse sources that haven't been
    extended to track the source-literal spelling.
    """
    if arg.long_flag:
        return arg.long_flag
    return _flag_for(arg.name)


class DispatchCommand(Struct, frozen=True):
    command: str
    command_args: dict[str, Any]
    schema: CommandSchema

    @property
    def argv(self) -> list[str]:
        """Argparse-shaped argv reconstructed from `command_args` per `schema`.

        For each argument in `schema.arguments` that appears in
        `command_args`, emit the appropriate token(s):

        - `positional`, no `nargs` → bare value.
        - `positional`, `nargs == "?"` → bare value when present (an
          absent zero-or-one value never appears in `command_args`).
        - `positional`, `nargs` an int or `"+"`/`"*"` (fixed arity, or
          a variadic positional — argparse `nargs="+"`/`"*"`/
          `REMAINDER`, or native `*args`) → each element as a bare
          value, in order.
        - `flag` → `--name` when truthy, omitted when falsy.
        - `optional`, no `nargs` → `--name value`, omitted when
          value == default.
        - `optional`, `nargs` an int (fixed arity) → `--name` once,
          followed by all N values.
        - `repeated`, `nargs in ("+", "*")` → `--name value1 value2 ...`
          in one occurrence (argparse `nargs="+"`/`"*"` on a
          keyword-style arg).
        - `repeated`, `nargs is None` (`action="append"`) →
          `--name value` once per element.

        `"positional"` is reserved for args with no CLI flag at all —
        `VarPositional`/`OptionalPositional` sources always map there,
        never to `"repeated"` — so this method never has to guess
        whether a `"repeated"` arg has a flag to emit.

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
                if isinstance(arg.nargs, int) or arg.nargs in ("+", "*"):
                    out.extend(str(element) for element in value)
                else:
                    out.append(str(value))
            elif arg.kind == "flag":
                if value:
                    out.append(_flag_for_arg(arg))
            elif arg.kind == "optional":
                if isinstance(arg.nargs, int):
                    out.append(_flag_for_arg(arg))
                    out.extend(str(element) for element in value)
                elif arg.default is None or str(value) != arg.default:
                    out.extend([_flag_for_arg(arg), str(value)])
            elif arg.kind == "repeated":
                if arg.nargs in ("+", "*"):
                    out.append(_flag_for_arg(arg))
                    out.extend(str(element) for element in value)
                else:
                    for element in value:
                        out.extend([_flag_for_arg(arg), str(element)])
        return out
