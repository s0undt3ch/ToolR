# Groups & commands

Every CLI subcommand starts with a [`command_group`][toolr.command_group]
declaration and one or more functions decorated with either `@command`
or the legacy `@<binding>.command`. Toolr supports both styles and
they can be mixed within the same `tools/` directory.

## Minimal example (binding style)

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

## String-path style (`@command(group="…")`)

The alternative decorator skips the `group =` binding entirely and
points at the target group by dotted path string. Useful for splitting
a group's commands across files without exporting a shared
`CommandGroup` binding.

```python
--8<-- "docs/writing-commands/files/string-path-example.py"
```

The rules:

- `command_group("greeting", …)` can be used as a bare expression
  statement (no assignment). It registers the group at module
  scope.
- `@command(group="greeting")` attaches a command to the group with
  that exact dotted full path.
- `@command("explicit-name", group="…")` overrides the CLI name (the
  function name's underscores-to-hyphens conversion still applies
  when you don't pass an explicit name).
- Bare `@command` (no kwargs, no parens) is *not* valid — it has no
  group to attach to. Build fails with a clear "missing `group=`"
  error pointing at the offending function.

### Order independence

Toolr does a two-pass static parse: every `command_group(...)`
declaration is collected first, then every `@command(group=...)`
reference is resolved against the registry. The order files are
scanned in doesn't matter — `tools/a.py` can declare a group that
`tools/b.py` attaches commands to, regardless of which file the
parser visits first.

### Typo safety

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

Both decorator styles support an explicit name override:

- Binding style: `@group.command("my-custom-name")`
- String-path style: `@command("my-custom-name", group="…")`

## Which style to pick

- **Binding style** (`group = command_group(...)` + `@group.command`)
  reads naturally when all of a group's commands live in one file.
  It's the original toolr convention and remains fully supported.
- **String-path style** (`command_group("…")` + `@command(group="…")`)
  shines when a group's commands are spread across multiple files,
  or when you'd rather not export a shared binding. The trade-off:
  the link between command and group is a string, so typos cost a
  build error instead of a `NameError`.

Both styles emit identical CLI output — you can mix them freely.

Next: [Arguments →](arguments.md)
