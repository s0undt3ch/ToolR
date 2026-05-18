# Known bugs

## Outstanding

None tracked at the moment. See the
[GitHub issues](https://github.com/s0undt3ch/ToolR/issues) for the live
list.

## Closed

- **`arg()` metadata fully plumbed end-to-end** (closes #198).
  `aliases`, `metavar`, `env`, `hide`, `display_order`,
  `conflicts_with`, `requires`, and `help_section` all flow from the
  Python annotation through the static parser into clap. Path
  constraints renamed to `path_must_exist` / `path_must_be_file` /
  `path_must_be_dir`. Old names accepted with a deprecation warning.
- Positional `int` / `float` coercion: typed clap value-parsers
  serialise typed JSON, msgspec validates against function hints on
  the Python side.
- `bool = False` parameters render as no-value `--verbose` flags.
- `dry_run` parameters expose `--dry-run` on the CLI.
- Enum-typed defaults render their resolved member value
  (`[default: add]`, not `[default: <expr>]`).
- Nested groups (`docker.command_group("image")`) build a proper
  subcommand tree at the CLI surface.

If you hit something that doesn't match the documented behaviour,
file an issue.
