# Unsupported command_group binding forms: implementation plan

> **For agentic workers:** use superpowers:subagent-driven-development or superpowers:executing-plans.
> Steps use checkbox (`- [ ]`) syntax.

**Goal:** a `command_group` call the static parser can't bind fails the build, and `CommandsTester`,
with "unsupported command declarations", instead of vanishing.

**Architecture:** one shared supported-shape helper in `groups.rs`, a detector built on it, a new
`UnsupportedCommandKind::GroupBinding`, and a Python mirror in `testing/_groups.py`.

**Spec:** `specs/2026-11-06-group-binding-forms-design.md`

## Global Constraints

- Plain top-level `g = command_group(...)`, `g: T = ...` and bare `command_group(...)` build exactly
  as today.
- No manifest schema change; no `RUNNER_SCHEMA_VERSION` bump.
- The Rust and Python messages are byte-identical; the parity corpus proves it.
- Patch coverage at least 99% with real tests.
- Commits: Conventional Commits, no Co-Authored-By, `TOOLR_TRUST_ALL=1`, stage files by path.

## Review Focus

- `group_binding` and the detector use the one `supported_group_call` helper: no statement is both
  bound and reported, or neither.
- A call nested in a supported statement's arguments, and `command_group(...).command`.
- An alias that resolves to something that isn't toolr's `command_group` isn't reported.
- Python `ast.walk` order versus Rust source order with two offending calls on one line.
- Group problems come before command problems in both messages.

---

### Task 1: Rust detector

**Files:**

- Modify: `crates/toolr-core/src/parser/commands.rs` (`UnsupportedCommandKind::GroupBinding` and its `Display`)
- Modify: `crates/toolr-core/src/parser/groups.rs` (`supported_group_call`, `detect_unsupported_group_bindings`)
- Modify: `crates/toolr-core/src/parser/build.rs` (collect and chain)
- Test: unit tests in `groups.rs` and `build.rs`

**Interfaces:**

- `fn supported_group_call(stmt: &Stmt) -> Option<(Option<String>, &ExprCall)>`: the match
  `group_binding` makes today (var name or `None`, call). `group_binding` calls it, then keeps its
  `is_command_group_call` check.
- `pub fn detect_unsupported_group_bindings(module, module_path, names) -> Vec<UnsupportedCommand>`:
  `function` is `"command_group"`, `kind` is `GroupBinding`.
- `Display` text for `GroupBinding`: the sentence in the design's "Error" section, verbatim.
- [ ] Write the failing unit tests below; run `cargo test -p toolr-core parser::groups` and see them fail.
- [ ] Extract `supported_group_call`; run the existing `groups.rs` tests unchanged.
- [ ] Implement the detector: for each `module.body` statement, skip it when `supported_group_call`
      gives a call that `is_command_group_call`; otherwise `walk_stmt` with a `Visitor` that records each
      `Expr::Call` where `is_command_group_call` holds and then calls `walk_expr`. Descend into
      `FunctionDef` and `ClassDef`.
- [ ] In `build.rs::build_manifest_from_files` (the loop at the `unsupported` vector), push the
      detector's output to a new `unsupported_groups` vector; at the return, build
      `BuildError::UnsupportedCommands` from `unsupported_groups` followed by `unsupported`.
- [ ] Format: `rustfmt --edition 2024` on the three files only.

Unit tests in `groups.rs` (`detect_unsupported_group_bindings` over `parse_src`):

- `plain_top_level_bindings_are_not_reported`: the three shapes, expect empty.
- `tuple_target_reports_each_call`: `a, b = command_group("a","A","A"), command_group("b","B","B")`
  gives 2.
- `chained_target_is_reported`: `a = b = command_group(...)` gives 1.
- `walrus_is_reported`: `(g := command_group(...))` gives 1.
- `if_try_with_for_while_match_bodies_are_reported`: one group in each, 6 total.
- `function_and_class_bodies_are_reported`: `def make(): return command_group(...)` and a class body.
- `call_nested_in_an_expression_is_reported`: `command_group(...).command` gives 1.
- `aliased_call_under_an_if_is_reported`: `from toolr import command_group as cg` then `cg(...)` under `if`.
- `unrelated_call_under_an_if_is_not_reported`: a local `def cg(...)` called under `if` is empty.
- `bare_form_under_an_if_is_reported`.
- `supported_group_call_matches_group_binding`: for each statement in a module mixing every shape,
  `group_binding(..).is_some()` equals "not reported" for statements holding a group call.

Unit tests in `build.rs` (`build_error_for`):

- `group_binding_forms_fail_the_build`: a tuple and an `if` group in `tools/m.py`; expect the exact
  two-line message with `(2)`.
- `group_bindings_are_reported_before_commands`: a nested-`def` command earlier in the file than a
  walrus group; the group line comes first.

### Task 2: Python mirror

**Files:**

- Modify: `crates/toolr-py/python/toolr/testing/_groups.py` (`_supported_group_call`,
  `_unsupported_group_bindings`; `_group_calls` uses the helper)
- Modify: `crates/toolr-py/python/toolr/testing/_shapes.py` (`GROUP_BINDING`,
  `_unsupported_commands`)
- Modify: `crates/toolr-py/python/toolr/testing/_discovery.py` (`collected_command_groups` `Raises`)
- Test: `tests/testing/test_groups.py`

**Interfaces:**

- `_unsupported_group_bindings(tools_dir: Path) -> list[Unsupported]`: tuples of
  `(module, "command_group", GROUP_BINDING)`, source order within a module, `_python_files` order
  across modules.
- `GROUP_BINDING` equals the Rust `Display` text. Check for circular imports between `_shapes` and
  `_groups` before wiring; if `_groups` already imports `_shapes`, define the constant in `_groups`.
- [ ] Write the failing tests below; run `uv run pytest tests/testing/test_groups.py` and see them fail.
- [ ] Factor the statement match in `_group_calls` into `_supported_group_call(stmt)`.
- [ ] Implement `_unsupported_group_bindings`: build the tables as `_group_declarations` does,
      collect `id()` of the calls `_group_calls` returns, then `ast.walk` each tree and record every
      other `ast.Call` where `_is_command_group` holds, sorted by `(lineno, col_offset)`.
- [ ] `_unsupported_commands` returns the group problems followed by the command problems.
- [ ] Format: `uv run ruff format` and `uv run ruff check` on the touched files only.

Tests in `tests/testing/test_groups.py` (use the existing factory fixtures, imports top-of-file):

- `test_tuple_target_is_an_unsupported_declaration`: two lines, exact text.
- `test_walrus_and_chained_target_are_unsupported_declarations`.
- `test_group_under_if_try_with_is_an_unsupported_declaration`.
- `test_group_in_a_function_body_is_an_unsupported_declaration`.
- `test_alias_under_an_if_is_an_unsupported_declaration`.
- `test_unrelated_call_under_an_if_is_not_reported`.
- `test_group_bindings_come_before_commands`.

### Task 3: Parity corpus

**Files:**

- Create: the `tests/e2e/files/parity_corpus/` cases listed in the spec's Corpus section, each an
  `m.py` (plus `n.py` for `err_group_binding_ordering`). Fixtures stay data, not packages.
- Modify: none (`tests/e2e/test_parser_tester_parity.py` discovers cases by directory).
- [ ] Write the cases; run `uv run pytest tests/e2e/test_parser_tester_parity.py -k "group_tuple or group_chained
      or group_walrus or group_in_ or group_bare_in or group_alias_in_if or group_binding_ordering
      or group_local_def"`.
- [ ] Confirm each `err_` case fails before Tasks 1 and 2 (build exits 0) and passes after.
- [ ] Confirm every existing corpus case still passes: `uv run pytest tests/e2e/test_parser_tester_parity.py`.

### Task 4: Docs, release note

**Files:**

- Modify: `docs/writing-commands/testing.md`, `crates/toolr-py/python/toolr/_decorators.py`
  (`command_group` docstring)
- Regenerate: `skills/toolr-command-authoring/references/*.md` via `cargo xtask build-skill-refs`
- Create: release note via `toolr release-notes add`
- [ ] Add the group rule and the new sample line to `docs/writing-commands/testing.md`.
- [ ] One sentence in the `command_group` docstring: "`command_group(...)` must be a top-level
      assignment or bare statement; a call under an `if`, a `def`, a tuple target or a walrus fails the
      build." Run `cargo xtask build-skill-refs`, commit the regenerated references, and read
      `skills/toolr-command-authoring/SKILL.md` for prose that now contradicts the rule.
- [ ] `toolr release-notes add --text "**Breaking:** …"` naming each form and its fix, per the
      spec. Pre-check with `toolr release-notes check --base main`.
- [ ] `toolr pre-commit regen-doc-snippets` only if a captured snippet changed (expected: no).

### Task 5: Verification

- [ ] `cargo test -p toolr-core`, `cargo test -p toolr --test '*'`, `uv run pytest`.
- [ ] `mise run test` (umbrella; includes the skill-refs drift gate). Poll long runs every 30 to 60
      seconds.
- [ ] Dogfood: `cargo run -p toolr -- --help` in this repo's `tools/` still builds, since the repo
      scan found no offending call.
- [ ] Coverage: the `cargo llvm-cov` command from `CLAUDE.md` (lcov output),
      then `cargo xtask patch-coverage --base main`; `uv run coverage run -m pytest && uv run coverage report`.
- [ ] `prek run --all-files` and `git grep -i paddle` before committing.
- [ ] Archive the design and plan with `git mv` into `specs/archive/2026/` as the last commit before
      opening the PR.
