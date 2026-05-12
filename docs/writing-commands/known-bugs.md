# Known bugs

## Outstanding

- **`arg()` metadata only partially plumbed through the rust
  front-end.** Path constraints (`must_exist` / `must_be_file` /
  `must_be_dir`) work. `aliases`, `group` (mutual-exclusion),
  `choices`, `metavar`, `action`, and `nargs` are accepted by the
  Python `arg()` constructor but are silently ignored by the rust
  binary. Tracked in [issue #198](https://github.com/s0undt3ch/ToolR/issues/198).

## Closed

Issues #193 / #194 / #195 / #196 / #197 — all five GA-blockers
tracked here when the rewrite landed — have been resolved:

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
