# Known bugs

Nothing currently outstanding. Issues #193 / #194 / #195 / #196 / #197
— all five GA-blockers tracked here when the rewrite landed — have
been closed in:

- positional `int` / `float` coercion: typed clap value-parsers
  serialise typed JSON, msgspec validates against function hints on
  the Python side.
- `bool = False` parameters render as no-value `--verbose` flags.
- `dry_run` parameters expose `--dry-run` on the CLI.
- enum-typed defaults render their resolved member value
  (`[default: add]`, not `[default: <expr>]`).
- nested groups (`docker.command_group("image")`) build a proper
  subcommand tree at the CLI surface.

If you hit something that doesn't match the documented behaviour,
file an issue.
