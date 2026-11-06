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
resolves `command` (FU-1, `ToolrNames` and `binds_toolr_command` in `parser/commands.rs`):

- A bare name is toolr's `command_group` when it is spelt `command_group` (today's rule, kept so
  nothing that builds now stops building), or when import tracking resolves it to toolr's
  `command_group`: an `as` alias, a re-export from another scanned module, or an assignment such as
  `cg = toolr.command_group`.
- An attribute call `<x>.command_group(...)` keeps today's rule: any base. That covers
  `toolr.command_group`, `import toolr as t; t.command_group`, and the method form
  `parent.command_group(...)`.

The parser also reads a top-level annotated assignment with a value and a single name target,
`h: X = <call>`, exactly like `h = <call>`. The annotation is ignored.

`binds_toolr_command` becomes generic over the member name (`command` or `command_group`), not a
copy. The cycle and depth guards, the star-import rules and the `Binding` ordering all stay the same.
A `Binding::Unknown` result (a module the parser can't read) does not count as `command_group`.
Today such a name is skipped anyway, and guessing would add groups Python might not have.

## Python mirror

`toolr.testing._groups._group_calls` accepts the same statement shapes (`ast.AnnAssign` with a value
and a `Name` target). It accepts a bare-name callee when it is spelt `command_group`, or when the
imported module's globals bind that name to `toolr.command_group` by identity. `_shapes.py` already
uses that rule for `command`, and it relies on the module already being imported, as
`_unsupported_commands` does. Docstrings on both sides cross-reference each other, like the existing
mirrors.

## Out of scope

- Groups bound in shapes Python accepts but the parser still doesn't read: tuple targets
  (`a, b = cg(...), cg(...)`), a walrus (`:=`), or a binding under `if`/`try`. They are still skipped
  without an error. A loud error for "a call to toolr's `command_group` the parser can't read" would
  need its own rules for nested scopes, like FU-1's command errors. It is logged as a follow-up, not
  done here.
- Short attribute bases that aren't toolr (`click.command_group` doesn't exist; any `.command_group`
  attribute is still accepted as today).

## Tests

- Rust unit tests in `parser/groups.rs`:
    - an aliased import;
    - `import toolr as t` with `t.command_group`;
    - `cg = toolr.command_group`;
    - a re-export through another `tools/` module;
    - an annotated binding;
    - an annotated binding without a value (`g: CommandGroup`), which binds nothing;
    - a local `def cg(...)` whose call is not a group.
- Parity corpus, one case per shape (`tests/e2e/files/parity_corpus/`): `ok_group_aliased_import`,
  `ok_group_annotated_binding`, `ok_group_assigned_alias` and `ok_group_reexported_alias`. Each
  declares a described group with one command, so the manifest and `CommandsTester` must agree on
  both the group and the command. Also `err_group_aliased_no_description`: an aliased group with no
  description fails on both sides with the existing undescribed-group message. Today it builds
  silently, so this proves the tester's group checks see aliases.
- An integration test in `crates/toolr/tests/` that runs `toolr --help` against an aliased group and
  sees its command listed.

## Release note

A `fix` note: groups declared through an aliased `command_group` import, or with an annotated
assignment, now appear in the manifest with their commands. Before this fix, both were dropped
without an error.

## Closed gap: aliases in `CommandsTester` (FU-35)

`CommandsTester` first resolved an alias by runtime identity against each imported module's
globals. That disagreed with the parser (an alias re-exported from outside `tools/`, a `try`/`except`
fallback binding, a `del`) and could not see aliases at all in the contradictory-arguments check,
which runs before anything is imported.

`_groups.py` now resolves aliases statically over the `tools/` module sources, mirroring
`binds_toolr_member`, `ImportTable` and `Assigned` rule for rule: same import, re-export and
assignment following, local binding wins, star imports, `MAX_REEXPORT_DEPTH` of 8, a module outside
`tools/` is `Unknown` and not counted, `del` and a non-`TYPE_CHECKING` `if` change nothing. No module
needs importing, so the contradictory check sees aliases too, with the build's message. The parity
corpus pins each shape (`*alias*` cases) so the two sides can't drift apart.

`Assigned` also reads a top-level annotated assignment with a value (`cg: object = command_group`)
like a plain one. That applies to `command` as well as `command_group`, as both go through
`binds_toolr_member`.
