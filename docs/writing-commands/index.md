# Writing commands

The authoring guide for tools-authors. Read in order, or jump to the
section you need:

1. [Groups & commands](groups.md) — declaring a `command_group` and
   attaching commands to it via `@command(group="…")`.
2. [Arguments](arguments.md) — turning function parameters into CLI
   arguments via type hints (positional / optional / flag /
   `Literal[...]` / `Enum`).
3. [Docstrings](docstrings.md) — Google-style docstrings drive
   `--help` output.
4. [Using `ctx`](context.md) — print, run subprocesses, prompt for
   input, exit cleanly.
5. [Annotations](annotations.md) — `arg()` for aliases, choices,
   mutually exclusive groups.
6. [Nested groups](nesting.md) — multi-level command hierarchies.
7. [Known bugs](known-bugs.md) — GA-blocking gaps in the current
   rust-front-end build.

Every example on these pages is a real file under
[`docs/writing-commands/files/`](https://github.com/s0undt3ch/ToolR/tree/main/docs/writing-commands/files)
that toolr can actually execute against the documentation fixture.

!!! warning "Migrating from the legacy decorators?"
    If your project still uses the old `group = command_group(...)`
    + `@group.command` style, see the
    [migration guide](../migration.md). The legacy decorators are
    deprecated and will be removed in toolr 1.0; every legacy call
    emits a runtime warning telling you exactly where to edit.
