---
name: toolr-command-authoring
description: |
  Author toolr commands in a project's own `tools/*.py` files. Use when
  adding, editing, or refactoring a toolr command, group, or context
  hook; when introducing a new `tools/` directory; when wiring
  `@command`, `@command_group`, `@arg`, or `@arg_section` decorators;
  when configuring a command's docstring-driven `--help`; or when
  debugging "command not found" / "manifest stale" errors against
  toolr. Triggers on phrases like "add a toolr command", "extend
  toolr", "wire a new toolr group", "toolr tools/", `@command_group`,
  `ctx.run`, `toolr-manifest.json`. Stays inert in projects that don't
  use toolr and on requests to package commands as a distributable
  plugin (covered by the `toolr-command-packaging` skill).
---

# Authoring toolr commands

You are extending an existing toolr project — a repo that already has
(or will have) a `tools/` directory at the root and a `toolr` binary
on PATH. Your job is to add or change `tools/*.py` files so the user
gets new subcommands under `toolr ...`.

This skill teaches the **shape** of authoring toolr commands. For the
**actual** surface of decorators, `Context` methods, and docstring
conventions, consult the generated references in `references/` —
they are rebuilt from toolr's own source on every release, so they
cannot drift.

## Workflow

1. **Confirm the project is a toolr project.** Look for `tools/` at
   the repo root with `tools/pyproject.toml`. If it doesn't exist,
   ask the user to run `toolr project init` first — that scaffolds
   everything correctly and you should not duplicate it.
2. **Decide where the command goes.** One file per subcommand is the
   easy default; group multiple commands in the same file when they
   share helpers or a parent group.
3. **Declare (or import) a group** with `command_group(...)`. The
   group is what `toolr <group> <cmd>` selects on. Reuse an existing
   group across files by passing `group="..."` to `@command` rather
   than redeclaring it.
4. **Write the function.** Take `ctx: Context` as the first
   parameter, then your CLI arguments as ordinary Python parameters.
   Type hints drive argparse binding; defaults make arguments
   optional; `Annotated[T, arg(...)]` adds clap metadata
   (`aliases`, `metavar`, `help_section`, `must_exist`, etc.).
5. **Document via Google-style docstring.** The first line is the
   short help (`toolr <group> --help`). The rest is the long help
   (`toolr <group> <cmd> --help`). `Args:` populates per-argument
   help.
6. **Try it.** `toolr <group> <cmd> --help` builds the manifest on
   the fly if it's stale (the freshness work landed in 0.20.0); on
   older toolr fall back to `toolr project manifest rebuild`. If the
   command doesn't appear, the manifest builder rejected it — read
   the error.

## What "looks right" looks like

A canonical single-file command:

```python
"""Long-form group description, used as the group's `--help` long text."""

from toolr import Context, arg, command, command_group

command_group("greet", "Say hello in various ways", docstring=__doc__)


@command(group="greet")
def hello(ctx: Context, who: str = "world", *, loud: bool = False) -> None:
    """Print a greeting.

    Args:
        who: Who to greet. Defaults to ``"world"``.
        loud: Shout instead of speak.
    """
    msg = f"Hello, {who}!"
    if loud:
        msg = msg.upper()
    ctx.info(msg)
```

Add this to `tools/greet.py`, then `toolr greet hello --help` works.

## Common authoring moves

- **Group across files.** Pass `group="ci.build"` to `@command` from
  any file; only one file needs to call `command_group("ci.build",
  ...)`.
- **Nested subgroups.** A dotted name (`command_group("docker.image",
  ...)`) attaches under the parent named before the last dot.
- **Help sections.** Build an `ArgSection` at module scope via
  `arg_section("Logging", description="...")`, then attach it via
  `Annotated[bool, arg(help_section=LOGGING)]`. Same `ArgSection`
  object across all members or you'll silently create duplicate
  sections.
- **Calling subprocesses.** Use `ctx.run(...)`; it inherits stderr
  for TTY-aware tools and propagates timeouts.

## Static-only discovery contract

toolr discovers commands **only by static analysis** of `tools/*.py` —
it never imports or executes your modules to build the manifest. Declare
`command_group(...)` at module top level and apply `@command` /
`@group.command` to module-level functions. Commands registered
dynamically — in a `for` loop, behind an `if`, or returned from a
factory called at import time — are **not** discovered and will not
appear in `--help`, completion, or dispatch. If a command is missing,
make its registration a top-level, statically-visible declaration.

## Anti-patterns

- **Don't register commands dynamically.** A loop like
  `for name in names: group.command(...)` or a factory that builds
  commands at import time produces nothing — the static parser can't see
  it. Declare each command at module level (see the static-only contract
  above).
- **Don't bypass the decorator surface.** Defining a function and
  then calling `register(...)` directly skips the manifest builder.
  Always `@command` or `@<group>.command`.
- **Don't reach into `toolr._*` internals.** Those modules are
  implementation detail; the public surface is exactly the names in
  `from toolr import (...)`, which `references/commands.md` lists.
- **Don't pass `description=` and `docstring=` to the same
  `command_group(...)`.** It raises. Pick one — `docstring=__doc__`
  is the canonical form when the module's docstring is the long
  description.
- **Don't write your own `argparse` subparser.** toolr owns the
  parser; you describe shape via decorators and type hints.

## References

- [`references/commands.md`](references/commands.md) — every name
  exposed by `import toolr`. Signatures, defaults, annotations, and
  docstrings, regenerated from `toolr.__all__` on every release.
  Treat it as the source of truth for the decorator API.
- [`references/docstrings.md`](references/docstrings.md) — exactly
  which Google-style section headers toolr's docstring parser
  recognises and how each is rendered. Generated from the same
  `KNOWN_SECTION_HEADERS` table the parser reads at runtime.

## Local feedback loop

- `toolr <group> --help` — list commands in the group; if yours is
  missing, the manifest builder rejected it.
- `toolr <group> <cmd> --help` — full per-command help.
- `toolr project manifest rebuild --force` — bypass freshness
  detection and rebuild from scratch.
- On a manifest error, the message points at the offending line in
  `tools/*.py`. Fix the source; the manifest auto-rebuilds on the
  next dispatch.

## Packaging is a different problem

If the user wants to **ship** an existing set of toolr commands as a
distributable Python plugin (so other projects can `pip install` and
get the commands), that is the
[`toolr-command-packaging`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-command-packaging)
skill's job. This skill does not cover wheel-building, manifest
embedding, or PyPI publishing — invoke the packaging skill for that
work.

## CI is a different problem

If the user wants to **run** these commands in GitHub Actions
(a caller workflow that installs toolr, sets up the venv, and
runs `toolr <group> <cmd>`), that is the
[`toolr-ci-setup`](https://github.com/s0undt3ch/toolr/tree/main/skills/toolr-ci-setup)
skill's job. This skill does not cover the `s0undt3ch/ToolR`
action, pinning policy, or CI cache shapes — invoke the
CI-setup skill for that work.
