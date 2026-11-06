# Unsupported command_group binding forms: design

Follow-up FU-36.

## Problem

The static parser binds a group only for a top-level statement of one of three shapes
(`crates/toolr-core/src/parser/groups.rs::group_binding`): `g = command_group(...)`,
`g: T = command_group(...)`, and a bare `command_group(...)`. Any other place a `command_group` call
can sit is skipped without a word. The group and every `@g.command` under it vanish from the
manifest, and the build exits 0. Python imports the same module without complaint, so nothing says
why the commands are missing.

The silent skip is the bug. toolr already settled how to treat a declaration it can't read without
running it (#674): reject it, on both sides, with "unsupported command declarations". A group
declaration the parser can't see belongs under the same rule.

## Current behaviour

Reproduced on `main` (dev binary, `toolr --help`, then the manifest; `CommandsTester.discover()` then
`collected_command_groups()`). Each case is a module with one group and one `@g.command` per group.

| Form | `toolr` (Rust) | `CommandsTester` |
| --- | --- | --- |
| `a, b = command_group(...), command_group(...)` | exit 0, no groups, no commands | builds `ga: [one]`, `gb: [two]` |
| `(g := command_group(...))` | exit 0, no groups, no commands | builds `g: [one]` |
| `a = b = command_group(...)` | exit 0, no groups, no commands | builds `g: [one]` |
| `if sys.platform:` then `g = command_group(...)` | exit 0, no groups, no commands | builds `g: [one]` |
| `try:` then `g = command_group(...)` | exit 0, no groups, no commands | builds `g: [one]` |
| `with ...:` then `g = command_group(...)` | exit 0, no groups, no commands | builds `g: [one]` |
| `for ...:` then `g = command_group(...)` | exit 0, no groups, no commands | builds `g: [one]` |
| `if sys.platform:` then bare `command_group(...)` | exit 0, no group | builds `g: []` |
| `g = command_group(...)` (control) | `g: [one]` | `g: [one]` |

The tester sees what Python runs, so it accepts every form. The two sides disagree: the tester
reports a healthy project that `toolr` then ships without its commands.

## Decision

Reject every `command_group` call the parser can't bind. Support none of the extra forms.

A `command_group(...)` call is supported only as the whole value of a module-level statement of the
three shapes above. Any other `command_group` call in a `tools/` module is an unsupported
declaration: inside an `if`, `try`, `with`, `for`, `while` or `match`, inside a `def` or class body,
in a tuple or chained target, in a walrus, or nested in a larger expression such as
`command_group(...).command`.

Why reject rather than support:

- Tuple, chained and walrus bindings are rare, and each has a plain rewrite
  (`a = command_group(...)` on its own line). Supporting them means the parser must pair targets
  with values positionally, with starred targets and mismatched arity as edge cases, for no capability
  the plain form lacks.
- A group under `if`/`try`/`with` is a conditional declaration. The manifest is static, so the group
  would exist or not by a branch toolr can't evaluate. That is the same reason #674 rejects commands
  under `if`. Accepting the group while rejecting the commands under it would be incoherent.
- Matching what `command_group` already allows keeps one rule to document: "declare groups and
  commands at the top level of the module".

Walking `def` and class bodies too is deliberate: `def make(): return command_group(...)` called as
`g = make()` is the same silent drop. A scan of this repo's `tools/`, `examples/`, `docs/.fixtures/`
and `skills/` finds no `command_group` call outside the supported shapes, so the tree itself doesn't
change. (Calls elsewhere are in `tests/` and toolr's own `_decorators.py`, which no `tools/` scan
reads.)

### Error

One new `UnsupportedCommandKind::GroupBinding`, reported through the existing
`BuildError::UnsupportedCommands` ("unsupported command declarations (N):"). There is no `def` to
name, so the name column carries the literal `command_group`:

```text
unsupported command declarations (2):
  - tools.m::command_group: `command_group(...)` must be a top-level `name = command_group(...)` assignment or a bare statement, which toolr reads without running it.
  - tools.m::command_group: `command_group(...)` must be a top-level `name = command_group(...)` assignment or a bare statement, which toolr reads without running it.
```

One line per offending call, so `a, b = command_group(...), command_group(...)` gives two. No line
numbers: the Rust side has byte offsets and the Python side line numbers, and keeping the two
messages byte-identical is worth more than the pointer. The module name and the count narrow it
enough.

Order: every group-binding problem (in `py_files` build order, source order within a module), then
every command problem. Rust keeps them in a separate vector and chains it first; the Python side
does the same. This avoids interleaving per module, which would need the tester to merge two
differently sorted walks.

Where the check sits is unchanged: `BuildError::UnsupportedCommands` is returned after contradictory
groups and name conflicts, and before type errors. `collected_command_groups` raises it first.

### What counts as `command_group`

Reuse each side's existing predicate, so nothing new can disagree:

- Rust: `groups.rs::is_command_group_call(call, names, module_path)`.
- Python: `testing/_groups.py::_is_command_group(func, tables, module, memo)`.

That is the literal name `command_group`, an alias the import tables resolve to it, and any
`<x>.command_group(...)`. A call to an unrelated function, including a local `def cg(...)`, isn't one.

### Rust shape

New `detect_unsupported_group_bindings(module, module_path, names) -> Vec<UnsupportedCommand>` in
`groups.rs`, next to `extract_groups` whose statement match it must stay in step with. It walks
`module.body` itself:

- A statement of a supported shape (the same match `group_binding` makes: single-`Name` `Assign`,
  `AnnAssign` with a `Name` target and a value, `Stmt::Expr` holding a `Call`) whose call is
  `command_group` is skipped whole.
- Every other statement goes to a `Visitor` that, for each `Expr::Call` satisfying
  `is_command_group_call`, records one problem, then keeps walking (so a call nested in a call counts).
  It descends into `def` and class bodies, unlike `DocBinding`.

`group_binding` and the new detector must share the supported-shape test, so extract the match into
one helper (`supported_group_call(stmt) -> Option<&ExprCall>`) that both call. Then the two cannot
drift: a statement is either bound or reported.

### Python shape

New `_unsupported_group_bindings(tools_dir) -> list[Unsupported]` in `testing/_groups.py`, reusing its
`_python_files`, `_ImportTable`, `_is_command_group` and the `_group_calls` statement match (factored
the same way). It walks every node with `ast.walk`, collects the offending calls, and sorts them by
`(lineno, col_offset)` to get source order. `_shapes._unsupported_commands` returns
`_unsupported_group_bindings(tools_dir) + <existing list>`. The message constant `GROUP_BINDING` sits
beside `NESTED` in `_shapes.py`, with the same "mirrors" comment.

`ast.walk` reaches the call inside a supported statement too, so skip the call objects that
`_group_calls` already returned (by `id`), not the statements.

## Corpus

New cases under `tests/e2e/files/parity_corpus/`, each with an `m.py` that also declares a command
on the group so a silent drop would be observable:

- `err_group_tuple_target`: `a, b = command_group(...), command_group(...)`. Two lines.
- `err_group_chained_target`: `a = b = command_group(...)`.
- `err_group_walrus`: `(g := command_group(...))`.
- `err_group_in_if`: `if sys.platform:` then `g = command_group(...)`, command under `@g.command`.
- `err_group_bare_in_if`: a bare `command_group(...)` under an `if`, a string-path `@command(group=...)`.
- `err_group_in_try`
- `err_group_in_with`
- `err_group_in_function`: `def make(): return command_group(...)`, then `g = make()`; the call in
  `make` is the finding.
- `err_group_alias_in_if`: `from toolr import command_group as cg`, call under an `if`; pins that the
  alias predicate is shared.
- `err_group_binding_ordering`: `m.py` has a nested-`def` command before a tuple group, `n.py` has an
  `if` group; the expected message lists both group lines before the command line.
- `ok_group_local_def_in_if`: a local `def cg(...)` called under an `if` is not a group; the module's
  real top-level group still builds.

## Release note

Needed: a build that exited 0 now fails. Start it with `**Breaking:**`. The affected projects were
already shipping a manifest without those groups, but their CI (`toolr self build-manifest --check`)
and `toolr` invocations start failing until the declaration is rewritten, which is what the Breaking
convention is for. Name each form and its fix (move to its own top-level `name = command_group(...)`
line; hoist out of `if`/`try`/`with`).

## Docs

- `docs/writing-commands/testing.md` ("unsupported command declarations" section): add the group
  rule and the new line to the sample output.
- `crates/toolr-py/python/toolr/_decorators.py::command_group` docstring: one sentence, then
  `cargo xtask build-skill-refs` to regenerate
  `skills/toolr-command-authoring/references/*.md`. Check `skills/toolr-command-authoring/SKILL.md`
  prose by hand.
- `collected_command_groups` docstring: add the group-binding case to `Raises`.

## Out of scope

- Supporting any of the forms. A later change can lift a form by teaching `supported_group_call`
  and the Python mirror about it, plus an `ok_` case.
- Reporting a `@g.command` whose `g` is unbound (it stays silent today; with groups now rejected the
  common cause is gone).
- Line numbers in the message.

## Open questions

1. Descend into `def`/class bodies (as above), or only module-level compound statements? The first
   catches factories; the second is closer to the literal reports. Default: descend.
2. `**Breaking:**` prefix, or a plain fix note? Default: Breaking.
