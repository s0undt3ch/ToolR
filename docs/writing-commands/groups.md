# Groups & commands

Every CLI subcommand starts with a [`command_group`][toolr.command_group]
declaration and one or more functions decorated with
[`@command`][toolr.command].

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

- `command_group("example", title="Example", description="…")`
  registers a top-level group. The first argument (`"example"`) is
  the invocation name; the title and description appear in `--help`.
  No assignment needed — toolr's static parser picks up the call as
  a module-level statement.
- `@command(group="example")` attaches the decorated function to the
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

## Order independence across files

Toolr does a two-pass static parse: every `command_group(...)`
declaration is collected first, then every `@command(group=...)`
reference is resolved against the registry. The order files are
scanned in doesn't matter — `tools/a.py` can declare a group that
`tools/b.py` attaches commands to, regardless of which file the
parser visits first.

## Overriding the CLI name

To register a command under a name different from its function:

```python
@command("snippet-checker", group="example")
def check_snippets(ctx): ...
# → toolr example snippet-checker
```

## Typo safety

If `@command(group="ci.helm-fdif")` references a group that doesn't
exist, manifest-build fails with the nearest match suggested:

```text
error: unknown group references (1):
  - tools.gh_actions::check-snippets: references group `ci.helm-fdif`
    which has no `command_group(...)` declaration. Did you mean `ci.helm-diff`?
```

## Function-name-to-command-name conversion

```python
--8<-- "docs/writing-commands/files/function-name-conversion.py"
```

Each function name with underscores becomes a hyphenated CLI command:
`simple_function` → `toolr names simple-function`,
`function_with_underscores` → `toolr names function-with-underscores`.

!!! warning "Legacy decorator deprecated"
    Toolr still accepts the older binding-style decorator
    (`group = command_group(...)` + `@group.command`) so existing
    projects keep running, but every legacy call emits a
    `ToolrDeprecationWarning` at runtime. The legacy form will be
    removed in toolr 1.0. See the [migration guide](../migration.md)
    for the (short) rewrite recipe.

Next: [Arguments →](arguments.md)
