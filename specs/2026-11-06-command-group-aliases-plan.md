# command_group aliases and annotated bindings: implementation plan

> **For agentic workers:** use superpowers:subagent-driven-development or superpowers:executing-plans.
> Steps use checkbox (`- [ ]`) syntax.

**Goal:** the static parser and `CommandsTester` read groups bound through an aliased
`command_group` or an annotated assignment, so their commands stop vanishing.

**Architecture:** generalise `binds_toolr_command` to a member name and reuse it from
`groups.rs`; accept `Stmt::AnnAssign`; mirror both in `toolr/testing/_groups.py`.

**Tech Stack:** Rust (`ruff_python_ast`), Python 3.11+ `ast`, pytest.

**Spec:** `specs/2026-11-06-command-group-aliases-design.md`

## Global Constraints

- `command_group` spelt literally, and any `<x>.command_group(...)`, keep working exactly as today.
- `Binding::Unknown` does not count as `command_group`.
- No manifest schema change; no `RUNNER_SCHEMA_VERSION` bump.
- Patch coverage ≥ 99% with real tests (no mocks).
- Commits: Conventional Commits, no Co-Authored-By; worktree commits need `TOOLR_TRUST_ALL=1`.

## Review Focus

- A local `def command_group(...)` or `def cg(...)`: the literal spelling still counts (today's
  rule); an alias that resolves to a local def must not.
- `from toolr import *` then `command_group(...)`: still found (literal spelling).
- An annotated target without a value (`g: CommandGroup`): no binding, no panic.
- `parent.command_group(...)` where `parent` is itself an aliased group: the parent path resolves.
- Cross-file: a group bound by alias in `tools/a.py`, used as `parent=` in `tools/b.py`.

---

### Task 1: Rust parser

**Files:**

- Modify: `crates/toolr-core/src/parser/commands.rs` (`ToolrNames`, `binds_toolr_command`, `toolr_member`)
- Modify: `crates/toolr-core/src/parser/groups.rs` (`extract_groups`, `group_binding`, `is_command_group_call`)
- Modify: `crates/toolr-core/src/parser/build.rs` (pass the import tables to `extract_groups`)
- Test: unit tests in `groups.rs`; `crates/toolr/tests/` integration test

**Interfaces:**

- `binds_toolr_member(all_imports, module, name, member: &str, depth) -> Binding` replaces
  `binds_toolr_command`; `ToolrNames::command(name)` calls it with `"command"`, new
  `ToolrNames::command_group(name)` with `"command_group"`. `ToolrNames` becomes `pub(super)`.
- `extract_groups(module, module_docstring, global_vars, names: ToolrNames)`.
- [ ] Write the failing unit tests listed in the spec's Tests section; run
      `cargo test -p toolr-core parser::groups` and see them fail.
- [ ] Implement; the bare-name check is `name == "command_group" || names.command_group(name) == Binding::Toolr`.
- [ ] Accept `Stmt::AnnAssign { target: Name, value: Some(call), .. }` in `group_binding`.
- [ ] Integration test: `toolr --help` in a project whose group is `from toolr import command_group as cg` lists its command.
- [ ] `cargo test -p toolr-core && cargo test -p toolr --test '*'` pass.
- [ ] Commit `fix(parser): read command_group aliases and annotated group bindings`.

### Task 2: Python tester mirror + parity corpus

**Files:**

- Modify: `crates/toolr-py/python/toolr/testing/_groups.py` (`_group_calls`, `_group_declarations`)
- Create: `tests/e2e/files/parity_corpus/{ok_group_aliased_import,ok_group_annotated_binding,ok_group_assigned_alias,ok_group_reexported_alias,err_group_aliased_no_description}/`

**Interfaces:**

- Consumes: Task 1's parser behaviour (the corpus runs the real binary).
- `_group_calls(tree, namespace: dict[str, Any])`: namespace from the imported module, like `_unsupported_commands`.
- [ ] Add the corpus cases; run `uv run pytest tests/e2e/test_parser_tester_parity.py`; the new cases fail on main's tester.
- [ ] Mirror the statement and callee rules; cross-reference docstrings both ways.
- [ ] Parity test and `uv run pytest tests/testing` pass.
- [ ] `toolr release-notes add --text "…"` (fix note per spec); commit `fix(testing): …` and the note.

### Task 3: Archive

- [ ] `git mv` both spec files to `specs/archive/2026/`; fix the plan's Spec path; commit.
