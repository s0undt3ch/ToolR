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

A `command_group(...)` call is supported only as the whole value of a module-level
`name = command_group(...)`, `name: T = command_group(...)` or bare `command_group(...)` statement,
with a string-literal name (positional or `name=`). Any other call to toolr's `command_group` is an
unsupported declaration: part of a tuple or chained target or a walrus, under `if`/`try`/`with`/a
loop/`match`, inside a function, lambda, comprehension or class, nested in another expression (a
decorator, a call argument, `command_group(...).command`), or a supported statement whose name isn't
a string literal. A supported statement is skipped whole, annotation included.

Why reject rather than support:

- Tuple, chained and walrus bindings are rare, and each has a plain rewrite
  (`a = command_group(...)` on its own line). Supporting them means pairing targets with values
  positionally, with starred targets and mismatched arity as edge cases, for no capability the plain
  form lacks.
- A group under `if`/`try`/`with` is a conditional declaration. The manifest is static, so the group
  would exist or not by a branch toolr can't evaluate, which is why #674 rejects commands under
  `if`. Accepting the group while rejecting the commands under it would be incoherent.
- One rule to document: declare groups and commands at the top level of the module.

Walking `def` and class bodies is deliberate: `def make(): return command_group(...)` called as
`g = make()` is the same silent drop. A scan of this repo's `tools/`, `examples/`,
`docs/.fixtures/` and `skills/` finds no call outside the supported shapes.

`command_group` creates a group when none is registered, so a name-only or title-only call is not a
harmless lookup. An exemption for calls that can't describe a group was tried and withdrawn for
that reason.

### What counts as `command_group`

The detector is deliberately stricter than the binding path: it reports, the binding path binds.

- Binding (`is_command_group_call` / `_is_command_group`, unchanged): the literal name
  `command_group`, an alias the import tables resolve to it, and any `<x>.command_group(...)`.
- Detector (`UnboundGroupCalls::is_toolr_group_call` / `is_toolr_group_call` in `_unbound_group_calls`):
  a name counts only when it resolves to toolr's `command_group` through the import tables
  (`Binding::Toolr` and its Python mirror); an attribute only when its base is the `toolr` module.
  A parameter of the enclosing `def` or `lambda` shadows the name, since that is the one rebinding
  needing no flow analysis; other local rebindings don't. So a local `def command_group`,
  `command_group = print`, a foreign import alias and `self.command_group(...)` are not reported.

A wrapper such as `def command_group(n, t, d): return _cg(n, t, d)` keeps its top-level
`g = command_group("w", ...)` binding, but the `_cg(n, t, d)` call inside the `def` is reported.

### Error

One new `UnsupportedCommandKind::GroupBinding`, reported through the existing
`BuildError::UnsupportedCommands` ("unsupported command declarations (N):"). There is no `def` to
name, so the name column is `command_group("<name>")` when the name argument is a string literal,
else `command_group`:

```text
unsupported command declarations (2):
  - tools.m::command_group("ga"): `command_group(...)` must be a top-level `name = command_group(...)` assignment or a bare statement, which toolr reads without running it.
  - tools.m::command_group("gb"): `command_group(...)` must be a top-level `name = command_group(...)` assignment or a bare statement, which toolr reads without running it.
```

One line per offending call, so `a, b = command_group(...), command_group(...)` gives two. No line
numbers: the Rust side has byte offsets and the Python side line numbers, and byte-identical messages
are worth more than the pointer.

Order: every group-binding problem (build order across modules, source order within one), then every
command problem. Rust keeps them in a separate vector and chains it first; Python prepends them. Source
order means position, not evaluation order: Rust walks with `SourceOrderVisitor`, Python sorts by
`(lineno, col_offset)`, so both messages match.

`BuildError::UnsupportedCommands` is returned after contradictory groups and name conflicts, and
before type errors. `collected_command_groups` raises it first.

### Rust shape

In `groups.rs`:

- `supported_group_call(stmt)` is the shape match (single-`Name` `Assign`, `AnnAssign` with a `Name`
  target and a value, `Stmt::Expr` holding a `Call`); `bound_group_call(stmt, names, module_path)`
  adds the `is_command_group_call` test. The binding pass (`group_binding`) and the detector both
  ask it, so a statement is bound or reported in one place.
- `detect_unsupported_group_bindings(module, module_path, names)` skips bound statements (reporting
  one whose name isn't a string literal) and sends every other statement to `UnboundGroupCalls`, a
  `SourceOrderVisitor` that records each toolr `command_group` call. It descends into `def`, lambda,
  comprehension and class bodies, and scopes parameter shadowing in `scoped`.
- `parse_group_call` reads the name through `argument(call, NAME)`, so `name="kw"` binds.
- `build.rs` chains `[unsupported_groups, unsupported].concat()`.

### Python shape

- `_groups.py`: `_supported_group_call`, `_bound_group_call` (used by `_group_calls` and the
  detector), `_unbound_group_calls` (explicit stack, so a very long expression can't raise
  `RecursionError`; sorted by position), `_is_toolr_module`, `_parameter_names`, `_import_tables`;
  `_group_declarations` reads the name with `_argument(call, _NAME)`.
- `_shapes.py`: `GROUP_BINDING` beside `NESTED`, `_group_call_label` and `_unsupported_group_bindings`
  (here rather than `_groups.py` because it needs the constant and `_shapes` already imports
  `_groups`); `_unsupported_commands` prepends its result.

## Corpus

Cases under `tests/e2e/files/parity_corpus/`, each declaring a command on the group so a silent drop
would be observable. Naming conventions are in that directory's `README.md`.

- Forms: `err_group_tuple_target` (two lines), `err_group_chained_target`, `err_group_walrus`,
  `err_group_in_if`, `err_group_bare_in_if`, `err_group_in_try`, `err_group_in_with`,
  `err_group_in_function`, `err_group_alias_in_if`, `err_group_toolr_attribute_in_if`,
  `err_group_described_call_in_body`, `err_group_wrapper_inner_call`.
- Name-only calls (reported): `err_group_name_only_in_body`, `err_group_name_only_creates_in_if`,
  `err_group_name_only_desc_none_in_if`, `err_group_name_only_rebind_existing_in_if`,
  `err_group_name_only_func_factory`, `err_group_name_only_bare_in_if_string_group`.
- Source order: `err_group_binding_source_order`, `err_group_source_order_augassign`,
  `err_group_source_order_ifexp`, `err_group_source_order_comprehension`,
  `err_group_source_order_for_target`, `err_group_source_order_kwargs`.
- Names: `ok_group_keyword_name`, `err_group_nonliteral_name`.
- Across modules: `err_group_ordering_groups_first`.
- Not reported: `ok_group_bound_statement_annotation`, `ok_group_local_def_in_if`,
  `ok_group_shadowed_by_local_def`, `ok_group_shadowed_by_parameter`,
  `ok_group_foreign_import_alias`, `ok_group_assigned_print`, `ok_group_foreign_attribute`.

## Release note

`**Breaking:**`, following the FU-2 duplicate-commands precedent: a build that exited 0 now fails,
and CI `toolr self build-manifest --check` starts failing until the declaration is rewritten. The
note names the forms and the fix (own top-level `name = command_group(...)` line; platform check
inside the command body) and omits the shadowing detail, which the docs carry.

## Docs

One statement of the rule: the doc comment on `detect_unsupported_group_bindings`. The `command`
docstring in `_decorators.py`, `docs/writing-commands/testing.md` and
`skills/toolr-command-authoring/SKILL.md` repeat it; the `command_group` docstring points at
`command`. `cargo xtask build-skill-refs` regenerates the references. `collected_command_groups`'s
`Raises` names the group-binding case.

## Out of scope

- Supporting any of the forms. A later change can lift a form by teaching `supported_group_call`
  and the Python mirror about it, plus an `ok_` case.
- Reporting a `@g.command` whose `g` is unbound (it stays silent today; with groups now rejected the
  common cause is gone).
- Line numbers in the message.
