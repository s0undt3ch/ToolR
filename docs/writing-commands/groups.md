# Groups & commands

Every CLI subcommand starts with a [`command_group`][toolr.command_group]
declaration and one or more functions decorated with `@group.command`.

## Minimal example

```python
--8<-- "docs/writing-commands/files/groups-example.py"
```

Place that file at `tools/example.py` in your repo. After running
`toolr project manifest rebuild` (or letting toolr rebuild
automatically on first invocation), `toolr example --help` lists
the new group's commands and `toolr example echo "hello, world"`
prints it.

## What's happening

- `command_group("example", title="Example", description="Example commands")`
  registers a top-level group. The first argument (`"example"`) is the
  invocation name; the title and description appear in `--help`.
- `@group.command` registers the decorated function as a subcommand.
  The function name (`echo`) becomes the CLI name. Underscores in the
  function name are converted to hyphens — `def render_diff` becomes
  `toolr example render-diff`.
- The first parameter (`ctx: Context`) is always the
  [`Context`][toolr.Context] object that toolr injects at execute
  time. It's never exposed as a CLI flag.
- The remaining parameters become CLI arguments inferred from their
  type hints and default values. Positional parameters without
  defaults become positional CLI args; parameters with defaults
  become optional flags. See [Arguments](arguments.md) for the full
  inference rules.

## Function-name-to-command-name conversion

```python
--8<-- "docs/writing-commands/files/function-name-conversion.py"
```

Each function name with underscores becomes a hyphenated CLI command:
`simple_function` → `toolr names simple-function`,
`function_with_underscores` → `toolr names function-with-underscores`.

To register under an explicit name, pass it as the decorator argument:
`@group.command("my-custom-name")`.

Next: [Arguments →](arguments.md)
