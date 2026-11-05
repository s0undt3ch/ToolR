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

## Static-only discovery

toolr discovers commands **only by static analysis** of your
`tools/*.py` files. It never imports or executes those modules to build
the command manifest — `toolr --help`, tab-completion, and first-run in a
fresh clone are all execution-free. Your repository's Python runs only
when you explicitly dispatch a command.

The practical contract: declare `command_group(...)` at module top level
and apply `@command` / `@group.command` to module-level functions, with
`group=` and any name as string literals. A command decorator on a nested
`def` — inside another function, a class, a `for` loop or behind an `if` —
or a non-literal `group=` or name fails the build with "unsupported command
declarations". Make each registration a top-level, statically-visible
declaration.

Each command name is defined once per group: two functions registering the
same name in one group, in one file or two, fail the build with "duplicate
commands".

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

## Declaring a group more than once

A group belongs to its first declaration. Calling `command_group` again
with the same name and parent returns that group, so several modules can
each declare `command_group("ci", "CI", "CI commands.")` and share it. A
later call that passes only the title, `command_group("ci", "CI")`, fetches
the group without repeating its description.

A later declaration with a different title or description doesn't change
the group. `toolr` keeps the first one and warns when it rebuilds the
manifest:

```text
toolr: warning: group `ci` is declared more than once; keeping the one in tools.ci and ignoring the different title in tools.release
```

Declare each group's title and description once, and fetch it by title
everywhere else.

"First" means first in file order to `toolr`, which reads `tools/` in
sorted path order. Python declares in import order instead, so a module
that imports a later-sorted one can see a different winner in tests. The
warning names both modules, so you can tell which declaration to remove.

A plugin can declare a group your `tools/` also declares. Your
declaration wins, without a warning.

Next: [Arguments →](arguments.md)
