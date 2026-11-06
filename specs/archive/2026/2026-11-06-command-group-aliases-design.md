# command_group aliases and annotated bindings: design

Follow-up FU-26.

## Problem

The static parser finds a group only when it sees a call spelt `command_group(...)` or
`<something>.command_group(...)`, and only in a plain `var = ...` assignment or a bare expression
statement (`crates/toolr-core/src/parser/groups.rs::group_binding` and `is_command_group_call`).
Two shapes that work at runtime are skipped:

```python
from toolr import command_group as cg
g = cg("alias", "Alias", "Aliased group.")       # alias: group skipped

h: CommandGroup = command_group("ann", "Ann", "Annotated group.")  # annotated: group skipped
```

The group is skipped, so every `@g.command` under it is skipped too. The build writes a manifest
without them and exits 0, and `toolr --help` never shows them. Python imports the same module
without complaint, so nothing tells the user why the commands are missing.

`CommandsTester` reads groups through the same static rules
(`crates/toolr-py/python/toolr/testing/_groups.py::_group_calls`), so its undescribed-group and
contradictory-group checks miss these groups too.

## Decision

The parser recognises a `command_group` call by what the callee resolves to, the way it already
resolves `command` (FU-1, `ToolrNames` and `binds_toolr_member` in `parser/commands.rs`):

- A bare name is toolr's `command_group` when it is spelt `command_group` (today's rule, kept so
  nothing that builds now stops building), or when import tracking resolves it to toolr's
  `command_group`: an `as` alias, a re-export from another scanned module, or an assignment such as
  `cg = toolr.command_group`.
- An attribute call `<x>.command_group(...)` keeps today's rule: any base. That covers
  `toolr.command_group`, `import toolr as t; t.command_group`, and the method form
  `parent.command_group(...)`.

The parser also reads a top-level annotated assignment with a value and a single name target,
`h: X = <call>`, exactly like `h = <call>`. The annotation is ignored.

`binds_toolr_member` (formerly `binds_toolr_command`) becomes generic over the member name (`command` or `command_group`), not a
copy. The cycle and depth guards, the star-import rules and the `Binding` ordering all stay the same.
A `Binding::Unknown` result (a module the parser can't read) does not count as `command_group`.
Today such a name is skipped anyway, and guessing would add groups Python might not have.

Resolution details that are easy to get wrong:

- **Memo.** One `ToolrNames` is built per manifest build and shared by every pass, so its memo is too.
  The memo is keyed on `(module, name, member, depth)`. Depth is part of the key because the depth
  cap (`MAX_REEXPORT_DEPTH`, 8) truncates a walk: a result computed near the cap can be `Other` where
  the same name resolved from depth 0 is `Toolr`, so it is only reusable at the same depth. The
  memo bounds the walk to one entry per module, name and depth, where modules that star-import each
  other would otherwise fan out exponentially.
- **Self-copy.** `cg = cg` (or `cg: T = cg`) copies whatever the name was before the statement, so
  it resolves against the imports, not against itself. Without that it would loop to the depth cap
  and read as `Other`.
- **Bare annotation.** `cg: T` with no value binds nothing, so it neither hides an imported `cg` nor
  records an assignment.
- **Annotated alias.** `cg: T = command_group` is followed like `cg = command_group`. That applies to
  `command` as well as `command_group`, as both go through `binds_toolr_member`.

## Python mirror

`CommandsTester` reads groups through `toolr.testing._groups`, which must reach the same verdicts as
the parser. It first resolved an alias by runtime identity against each imported module's globals.
That disagreed with the parser (an alias re-exported from outside `tools/`, a `try`/`except`
fallback binding, a `del`) and could not see aliases at all in the contradictory-arguments check,
which runs before anything is imported.

So `_groups.py` resolves aliases statically over the `tools/` module sources, rule for rule:

- `_ImportTable` mirrors `ImportTable` and `Assigned` (the statements recorded, a local binding
  winning over an import, star imports, annotated assignments), `_binding` mirrors
  `ToolrNames::binds_toolr_member` with the same `MAX_REEXPORT_DEPTH` and memo, and `_is_command_group`
  and `_group_calls` mirror `is_command_group_call` and `group_binding`.
- A module outside `tools/` is `Unknown` and not counted. `del` and a non-`TYPE_CHECKING` `if` change
  nothing.
- No module needs importing, so the contradictory check sees aliases too, with the build's message.
- Only `command_group` is resolved here, so the resolver drops the `member` parameter and its memo
  is keyed on `(module, name, depth)`.
- The tester still resolves `command` by runtime identity (`_command_decorators` in `_shapes.py`),
  unchanged by this work.

Docstrings on both sides cross-reference each other (`Mirrored in ...; change both together`). The
parity corpus is what proves the two sides agree: it runs the real binary against each shape
(`*alias*` cases), so they can't drift apart unnoticed. The hand-written resolver table in
`tests/testing/test_groups.py` is not run against the binary.

## Out of scope

- Groups bound in shapes Python accepts but the parser still doesn't read: tuple targets
  (`a, b = cg(...), cg(...)`), a walrus (`:=`), or a binding under `if`/`try`. They are still skipped
  without an error. A loud error for "a call to toolr's `command_group` the parser can't read" would
  need its own rules for nested scopes, like FU-1's command errors. It is logged as a follow-up, not
  done here.
- Short attribute bases that aren't toolr (`click.command_group` doesn't exist; any `.command_group`
  attribute is still accepted as today).

## Tests

- Rust unit tests in `parser/groups.rs` and `parser/commands.rs`, by category:
    - alias shapes: an aliased import, `import toolr as t` with `t.command_group`,
      `cg = toolr.command_group`, a re-export through another `tools/` module, and annotated aliases;
    - annotated bindings, with and without a value (`g: CommandGroup` binds nothing);
    - negatives: a local `def cg(...)`, an alias from an unreadable module, a `try`/`except` fallback
      binding, `del`;
    - self-copies (`cg = cg`), through an import, a star import and nothing at all, for
      `command_group` and `command`;
    - the memo: it agrees with a fresh cache and stays within modules x names x (depth cap + 1).
- Python unit tests in `tests/testing/test_groups.py`: the same shape categories through
  `_group_declarations`, plus the memo bound on `_binding`.
- Parity corpus (`tests/e2e/files/parity_corpus/`), one case per shape, each with a described group
  and a command so the manifest and `CommandsTester` must agree on both: aliased import, annotated
  binding, assigned alias, re-exported alias, annotated alias, self-copy, bare annotation, deleted
  alias, and an annotated `command`. The `err_*` cases (an aliased group with no description, an
  alias from outside `tools/`, a `try`/`except` fallback) fail identically on both sides: the alias is
  not read as a group, so a companion module's undescribed declaration is what errors. The first
  built silently before this work, so it proves the tester's group checks see aliases.
- An integration test in `crates/toolr/tests/` that runs `toolr --help` against an aliased group and
  sees its command listed.

## Release note

A `fix` note: groups declared through an aliased `command_group` import, or with an annotated
assignment, now appear in the manifest with their commands. Before this fix, both were dropped
without an error.
