# Parser and `CommandsTester` parity (FU-1, FU-2, FU-3)

- **Status:** accepted (open points decided 2026-11-04)
- **Plan:** [`2026-11-04-parser-tester-parity-plan.md`](./2026-11-04-parser-tester-parity-plan.md)
- **Prior work:** `archive/2026/2026-10-31-tester-standalone-group-design.md` (unknown-group parity),
  `archive/2026/2026-11-02-symlinked-tools-files-design.md` (same-text errors in both walks),
  `archive/2026/2026-09-29-plugin-parity-design.md`.

## Problem

The static parser (`crates/toolr-core/src/parser/`) decides what `toolr` runs. `toolr.testing.CommandsTester`
imports `tools/` and reads the runtime registry. Validated on `main` (`bf5648f6`), they disagree in three ways.

**FU-1, decorator shapes.** `commands.rs::extract_commands` reads only top-level `module.body` (`:47-52`).
`parse_binding_command` treats any `@X.command` as a group binding (`:228-239`), so `@toolr.command(group="grp")`
becomes var `toolr`, finds no binding and is skipped. `parse_string_path_command` requires the bare name `command`
(`:290,306`), so `from toolr import command as cmd; @cmd(group="grp")` is skipped. `group=` goes through `literal_str`
(`:276`): `group=NAME` becomes `None`, then `""`, then the misleading "`@command` is missing a `group=...` kwarg."
A command inside a nested `def` is never seen. Each case registers in the tester; the binary shows only the plain
command, or fails with the wrong message.

**FU-2, duplicates.** `build.rs:157,198` extends `all_commands` without checking `(group, name)`. A debug build panics
in clap on `toolr grp --help` (`Command grp: command name 'dup' is duplicated`); a release build silently keeps one.
`CommandGroup.command` (`_decorators.py:158-164`) logs at debug level and the last definition wins. Both shapes
reproduce: two defs in one module, the second with `name="dup"`; and two identical modules.

**FU-3, suggestion ties.** `build.rs::nearest_group` (`:567-580`) iterates a `HashSet` with a strict `<`, so equally
close groups give a random suggestion: for `aa` against `ab`, `ca` and `ac`, any of the three can come back. The
Python mirror (`testing/_unknown_groups.py:66-82`) takes the first in sorted order, and its docstring tells fixtures
to avoid ties.

## Decisions

### Decided open points (2026-11-04)

- **New error kind.** Nested-`def` commands, a non-literal `group=` and a non-literal command name share one new
  error, "unsupported command declarations", and `CommandsTester` raises it with identical text.
- **Duplicates.** A duplicate `(group, command)` is an error within one source tree: the manifest build (a project's
  `tools/` or one plugin package) and `CommandsTester`. Cross-tree duplicates (first-party against plugin, plugin
  against plugin) keep today's warnings.
- **Plain import.** Outside the tester, importing a module with a duplicate keeps last-one-wins.
- **Nested command whose outer function never runs.** Stays as described below: the build fails, the tester can't
  see it.
- **Tie fixture.** Groups `ab`, `ca` and `ac`, target `aa`, expects `ab`.
- **Release notes.** The duplicate and rejection note starts `**Breaking:**`; a second, non-breaking note covers the
  new decorator forms.

### FU-3: smallest name wins a tie

`nearest_group` keeps a candidate when `d < best_d`, or `d == best_d` and `candidate < best`. Rust `String` and Python
`str` both compare by code point, so this matches Python's sorted-first. Drop the "Ties differ from Rust … avoid ties"
paragraph from `_nearest_group`'s docstring.

### FU-2: a duplicate `(group, command)` is an error

**Rust.** After the unknown-group check in `build_commands_inner`, group `all_commands` by `(group, name)` in first-seen
order. Any key with two or more definitions is a `BuildError::DuplicateCommands`:

```text
duplicate commands (1):
  - `grp dup` is defined more than once: tools.m1::dup, tools.m2::dup. Rename all but one, e.g. with `name="…"`.
```

The path is the command as typed (`merge.rs::command_path`, made `pub(crate)`). Definitions are `module::function`
in build order: sorted file order, then source order. Module, function, group and name go through
`text::escape_control_chars`, like the other build messages. The check runs last, so a typo'd group reports as an
unknown group first.

**Python.** `CommandGroup.command`'s `register` is the one place both attach paths go through (the bound decorator and
the standalone `@command(group=...)`, direct or drained from pending). On a repeat name it appends
`DuplicateCommand(group, name, first, second)` to `_get_duplicate_command_storage()`, the same patch-point pattern as
`_get_pending_command_storage()`. `CommandsTester` patches it with its own list; `collected_command_groups()` raises
`ValueError` with the Rust text, built by `testing/_duplicates.py::_format_duplicate_commands`. Keys and definitions
are ordered by `_source_order_key` plus `co_firstlineno`, which equals the Rust build order.

Outside the tester the storage drops entries and the last definition still wins, as today. `toolr` has already refused
the tree by then, so nothing runs on that path.

**First-party against plugin.** Unchanged. `third_party/merge.rs` already resolves cross-source duplicates: a local
command shadows a plugin's (`Shadowed` warning), and a command two plugins define is disabled (`Conflict` warning),
with a winner-in-config follow-up tracked at `RESOLVE_ISSUE_URL`. A user can't edit an installed plugin, so a hard
error there would brick the repository. The new error covers duplicates inside one source tree: a project's `tools/`,
or one plugin package.

**Plugin packages.** `toolr self build-manifest <pkg>` goes through `build_commands`, so plugin authors get every new
error at build time. Installed fragments aren't re-validated; `merge.rs` already keeps the first copy of a command a
hand-edited fragment lists twice.

### FU-1: support the attribute and aliased forms, reject what can't be read

**Parser support.** `extract_commands` and `detect_name_conflicts` already have the module's `ImportTable`. A small
`ToolrNames` view over it answers two questions:

- `@X.command…` where `resolve_module_binding(X) == Some("toolr")` (`import toolr`, `import toolr as t`) is the
  string-path form, checked before the group-binding branch.
- `@N(...)` or `@N` where `N == "command"`, or `candidates(N)` holds `{module: "toolr", original_name: "command"}`
  (`from toolr import command as cmd`), is the string-path form.

`groups.rs::is_command_group_call` already accepts the `.command_group` attribute form, so `toolr.command_group(...)`
needs nothing.

**Parser rejections.** The parser has no error for either rejected shape: a nested command is silently dropped, and
`group=NAME` reports a missing `group=`. Both become `BuildError::UnsupportedCommands`, checked right after the
name-conflict check:

```text
unsupported command declarations (3):
  - tools.m::nested: `@command` must decorate a top-level `def`, which toolr reads without running it.
  - tools.m::nonlit: `group=` must be a string literal.
  - tools.m::renamed: the command name must be a string literal.
```

- *Nested:* any recognised command decorator on a `def` that isn't directly in `module.body`: inside another `def`,
  a class, or an `if`/`try`/`with`/`for`/`while`/`match` block. A binding decorator counts only when its variable
  resolves to a known group, as at top level.
- *Non-literal `group=`:* a `group=` keyword whose value isn't a string literal.
- *Non-literal name:* a positional name or `name=` that isn't a string literal. Same bug class (the parser derives the
  name from the function, the tester uses the value), and the same check covers it.

**Tester rejections.** At decoration time `group` is already a string, so only the source can tell `group="x"` from
`group=X`. `testing/_shapes.py` checks every registered or pending command whose `co_filename` is under
`search_path/tools` (commands a test body defines are skipped). It parses each file once with `ast` and finds the
`FunctionDef`/`AsyncFunctionDef` by name and `co_firstlineno` (the first decorator's line):

- not directly in `module.body` → the nested line;
- a decorator call with a `group=` or name argument that isn't an `ast.Constant` string → the matching line.

`collected_command_groups()` raises `ValueError` with the same text. Order of checks, in both: unsupported
declarations, then unknown groups, then duplicates. One parser asymmetry remains: a nested command that never runs
(the outer function is never called) fails the build but is invisible to the tester.

### Differential corpus

`tests/e2e/test_parser_tester_parity.py`, next to `test_unknown_group_parity.py` and `test_symlinked_modules.py`,
which already drive the binary (`toolr_bin`, `make_tools_project`) and the tester over one fixture. Each case is a
directory under `tests/e2e/parity_corpus/<case>/` of modules copied into `tools/`. The prefix says what both sides
must do, so a case both sides wrongly accept can't pass:

- `ok_*`: `toolr --help` exits 0 and the manifest it wrote gives `{group: sorted(names)}`, equal to the tester's
  `{_group_path(key): sorted(group.get_commands())}`. Empty groups count.
- `err_*`: `toolr --help` fails, the tester raises, and `f"toolr: {exc}\n"` is in stderr. One error kind per case, so
  check order doesn't decide the comparison.

Cases: `ok_plain`, `ok_toolr_attribute`, `ok_toolr_module_alias`, `ok_aliased_import`, `ok_cross_file_binding`,
`err_nested_def`, `err_nested_if`, `err_nonliteral_group`, `err_nonliteral_name`, `err_dup_one_module`,
`err_dup_two_modules`, `err_suggestion_tie` (groups `ab`, `ca`, `ac`, target `aa`; expects `ab`).

## Compatibility

- **Breaking.** A tree with a duplicate command, a nested command, or a non-literal `group=`/name used to build
  (silently dropping or picking a command) and now fails, in `toolr` and in `CommandsTester`. A suite that was green
  on those shapes turns red. The release note starts `**Breaking:**`, as F-17's symlink note did.
- **Feature.** `@toolr.command(...)`, `@t.command(...)` and `@cmd(...)` now register. A second, non-breaking note.
  Together this is a minor (`0.X.0`) release under CLAUDE.md's rules.
- No manifest field changes: `MANIFEST_SCHEMA_VERSION`, `RUNNER_SCHEMA_VERSION` and `SCHEMA_VERSION` stay.
- No `__all__` change. The `command` docstring and `CommandsTester.collected_command_groups`'s `Raises:` change, and
  both feed `skills/toolr-command-authoring/references/`, so `cargo xtask build-skill-refs` regenerates them.

## Docs

- `docs/writing-commands/testing.md` (`:143-146`): replace "accepts … but `toolr` doesn't register" with the supported
  forms and the shared errors; list duplicates under what `collected_command_groups()` raises.
- `_decorators.py::command` docstring: same rewrite of the "only one shape counts" paragraph.
- `docs/writing-commands/groups.md` and `across-files.md`: one sentence each, a `(group, name)` pair is defined once.
- `skills/toolr-command-authoring/SKILL.md`: check the prose against the new rules by hand.

## Out of scope

- Imperative registration (`g.command("x")(func)` outside a decorator): the tester registers it, the parser doesn't.
- `from toolr import command_group as cg`: `groups.rs` doesn't resolve aliases for groups either.
- A command whose name equals a subgroup's leaf in the same group.
- Cross-source duplicates beyond the existing warnings.
