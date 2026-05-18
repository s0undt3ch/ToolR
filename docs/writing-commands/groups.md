# Groups & commands

Every CLI subcommand starts with a [`command_group`][toolr.command_group]
declaration and one or more functions attached to it.

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

- `example = command_group("example", title="Example", description="…")`
  registers a top-level group and returns a binding you can attach
  commands to. The first argument (`"example"`) is the invocation
  name; the title and description appear in `--help`.
- `@example.command` attaches the decorated function to the
  registered group. The function name (`echo`) becomes the CLI name;
  underscores are converted to hyphens (`render_diff` →
  `toolr example render-diff`).
- The first parameter (`ctx: Context`) is always the
  [`Context`][toolr.Context] object that toolr injects at execute
  time. It's never exposed as a CLI flag.
- The remaining parameters become CLI arguments inferred from their
  type hints and default values. Positional parameters without
  defaults become positional CLI args; parameters with defaults
  become optional flags. See [Arguments](arguments.md) for the full
  inference rules.

## Overriding the CLI name

To register a command under a name different from its function:

```python
@example.command("snippet-checker")
def check_snippets(ctx): ...
# → toolr example snippet-checker
```

## Function-name-to-command-name conversion

```python
--8<-- "docs/writing-commands/files/function-name-conversion.py"
```

Each function name with underscores becomes a hyphenated CLI command:
`simple_function` → `toolr names simple-function`,
`function_with_underscores` → `toolr names function-with-underscores`.

## When you outgrow a single file

The bound-decorator form above is the canonical shape for tools that
live in one file: the `example` binding sits right next to the
commands it owns, so the relationship is obvious from a glance.

Tools tend to grow. When you want commands in `tools/foo.py` to
attach to a group declared in `tools/_common.py`, importing the
binding across files gets awkward. Toolr's *string-keyed* decorator
is built for that case:

```python
# tools/foo.py
from toolr import command

@command(group="example")
def run(ctx): ...
```

The static parser resolves the `group="example"` reference at
manifest-build time, so the file declaring `command_group("example", …)`
and the file declaring the command don't need to share an import.
Read [*Scaling command groups across files*](across-files.md) for
the full picture, including the typo-safety guarantees the
string-keyed form gives you.

Next: [Arguments →](arguments.md)
