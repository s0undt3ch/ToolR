# Parser and `CommandsTester` Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `toolr` and `CommandsTester` agree on which commands a `tools/` tree declares and fail it with the same text:
deterministic group suggestions (FU-3), duplicate `(group, command)` as an error (FU-2), `@toolr.command` and aliased
`command` imports supported, nested and non-literal declarations rejected (FU-1).

**Architecture:** Rust checks live in `parser/build.rs::build_commands_inner` (so `toolr self build-manifest` gets
them too) and `parser/commands.rs`. Python mirrors each check in `toolr.testing` and raises from
`collected_command_groups()`, as unknown groups already do. A differential corpus under `tests/e2e/` runs every
fixture through the dev binary and the tester.

**Tech Stack:** Rust (ruff_python_ast, thiserror), Python 3.11+ (`ast`, attrs), pytest.

**Spec:** [`specs/2026-11-04-parser-tester-parity-design.md`](./2026-11-04-parser-tester-parity-design.md)

## Global Constraints

- Every new message exists once in Rust and once in Python, each with a "mirrors …; change both together" note.
- `{function}` in Python messages is `func.__name__` (the bare `def` name Rust reports), never `__qualname__`.
- Error text escapes module, function, group and name with `text::escape_control_chars` /
  `_escape_control_chars`.
- Check order in both: unsupported declarations, unknown groups, duplicates.
- `third_party/merge.rs` behaviour is unchanged (local shadows plugin; plugin-plugin conflicts disable).
- No manifest or runner schema bump. No `__all__` change.
- Fixtures reproduce the validated scratch shapes exactly (`a` to `f`; the tie fixture uses `ca` in place of a third group).
- Real tests only, no mocks; factory fixtures for `tmp_path`; imports top of file; 99% patch coverage.
- Format touched Rust files only (no `cargo fmt -p`). Conventional Commits, no `Co-Authored-By`, stage by path.

## Review Focus

1. **Order parity.** Duplicate definitions and offending commands list in the same order from both sides (sorted
   files, then source line). Tasks 2 and 4.
2. **Recognition doesn't widen.** `@X.command` stays a group binding unless `X` is bound to the `toolr` module; a
   click-style `@cli.command` in a nested scope is not reported. Tasks 3 and 4.
3. **Tester scope.** Shape checks touch only functions whose file is under `search_path/tools`; commands defined in a
   test body still work. Task 4.
4. **Corpus can't pass vacuously.** `ok_*` requires exit 0 on both sides, `err_*` requires both to fail. Task 4.

---

### Task 1: FU-3, deterministic tie-break in `nearest_group`

**Files:**

- Modify: `crates/toolr-core/src/parser/build.rs` (`nearest_group`)
- Modify: `crates/toolr-py/python/toolr/testing/_unknown_groups.py` (`_nearest_group` docstring)
- Test: `build.rs` `#[cfg(test)]`

**Steps:**

- [ ] **Step 1: Test.** `nearest_group_breaks_ties_by_smallest_name`: registered `{"ab", "ca", "ac"}`, target `"aa"`
      → `Some("ab")`; build the set in several insertion orders and assert all agree.
- [ ] **Step 2: Run** `cargo test -p toolr-core nearest_group` → fails intermittently or outright.
- [ ] **Step 3: Implement.** Keep the candidate when `d < bd || (d == bd && candidate < best)`. Delete the "Ties
      differ from Rust … avoid ties" paragraph from `_nearest_group`.
- [ ] **Step 4: Run** Step 2 → PASS.
- [ ] **Step 5: Commit.** `fix(parser): break group-suggestion ties by the smallest name`

### Task 2: FU-2, duplicate `(group, command)` is an error

**Files:**

- Modify: `crates/toolr-core/src/parser/build.rs` (new `DuplicateCommand`, `BuildError::DuplicateCommands`,
  `format_duplicate_commands`, check at the end of `build_commands_inner`)
- Modify: `crates/toolr-core/src/third_party/merge.rs` (`command_path` → `pub(crate)`)
- Modify: `crates/toolr-py/python/toolr/_decorators.py` (`DuplicateCommand`, `_get_duplicate_command_storage`,
  `CommandGroup.command.register`)
- Create: `crates/toolr-py/python/toolr/testing/_duplicates.py` (`_format_duplicate_commands`)
- Modify: `crates/toolr-py/python/toolr/testing/_discovery.py` (collector, patcher, `collected_command_groups`)
- Test: `build.rs` tests, `crates/toolr/tests/cli_smoke.rs`, `tests/registry/test_integration.py`

**Interfaces:**

- `pub struct DuplicateCommand { pub group: String, pub name: String, pub definitions: Vec<(String, String)> }`
  (module, function). Message:

  ```text
  duplicate commands ({count}):
    - `{command_path}` is defined more than once: {m1}::{f1}, {m2}::{f2}. Rename all but one, e.g. with `name="…"`.
  ```

- `class DuplicateCommand(NamedTuple): group: str; name: str; first: Callable; second: Callable`.

**Steps:**

- [ ] **Step 1: Rust tests.** `duplicate_command_in_one_module_fails` (scratch `e`: `@g.command def dup` plus
      `@g.command(name="dup") def dup2`) and `duplicate_command_across_modules_fails` (scratch `f`: identical `m1.py`,
      `m2.py`) assert the full message. `three_definitions_list_all_three`. `unknown_group_reported_before_duplicate`.
      `build_commands` on a plugin package dir with a duplicate fails the same way. `cli_smoke.rs`: `toolr --help` on
      `f` exits non-zero with the message and no panic.
- [ ] **Step 2: Python tests.** `test_collected_command_groups_rejects_duplicate_in_one_module` and
      `..._across_modules` (same fixtures) assert the exact Rust text; a test that a duplicate from one tester doesn't
      reach the next. `tests/decorators/test_decorators_unit.py:149` (last wins outside the tester) stays valid.
- [ ] **Step 3: Run** `cargo test -p toolr-core duplicate` and `uv run pytest tests/registry -k duplicate` → FAIL.
- [ ] **Step 4: Implement.** Rust: group `all_commands` by `(group, name)` keeping first-seen key order; error if any
      key has ≥2 definitions. Python: in `register`, on a repeat name append to `_get_duplicate_command_storage()`
      (default returns a fresh list, like `_get_pending_command_storage`) before the existing override.
      `_format_duplicate_commands` merges entries per key and orders keys and definitions by
      `(_source_order_key(func, tools_dir), func.__code__.co_firstlineno)`. `collected_command_groups()` raises
      `ValueError` after the unknown-group check; add it to `Raises:`.
- [ ] **Step 5: Run** Step 3 → PASS; `uv run pytest tests/registry tests/decorators` → PASS.
- [ ] **Step 6: Commit.** `fix(parser)!: reject duplicate commands in a group` (Rust and Python together).

### Task 3: FU-1, recognise `@toolr.command` and aliased `command`

**Files:**

- Modify: `crates/toolr-core/src/parser/commands.rs` (`ToolrNames`, `command_decorator`, `parse_*_command`,
  `extract_commands`, `detect_name_conflicts`)
- Modify: `crates/toolr-core/src/parser/build.rs` (pass the module's `ImportTable` to `detect_name_conflicts`)
- Test: `commands.rs` and `build.rs` tests

**Interfaces:**

- `struct ToolrNames<'a>(&'a ImportTable)` with `is_toolr_module(&str) -> bool` (`resolve_module_binding == "toolr"`)
  and `is_command(&str) -> bool` (`"command"`, or a `candidates` entry `{module: "toolr", original_name: "command"}`).

**Steps:**

- [ ] **Step 1: Tests.** Scratch `a` (`import toolr`; `@toolr.command(group="grp")`), `import toolr as t` +
      `@t.command(group="grp")`, scratch `b` (`from toolr import command as cmd`; `@cmd(group="grp")`) each build both
      commands. `@toolr.command("x", name="y", group="grp")` reports a name conflict. `@other.command` with `other`
      neither a group nor `toolr` stays ignored.
- [ ] **Step 2: Run** `cargo test -p toolr-core parser::` → the new tests FAIL.
- [ ] **Step 3: Implement.** Thread `ToolrNames` into `command_decorator`. Test the string-path shapes (bare/call on
      a `command` name, attribute/call on a `toolr` module binding) before `parse_binding_command`.
- [ ] **Step 4: Run** Step 2 → PASS.
- [ ] **Step 5: Commit.** `feat(parser): recognise @toolr.command and aliased command imports`

### Task 4: FU-1 rejections in both, plus the differential corpus

**Files:**

- Modify: `crates/toolr-core/src/parser/commands.rs` (`UnsupportedCommand`, `detect_unsupported_commands`; flags
  `group_not_literal`, `name_not_literal` on `CommandDecorator::StringPath`/`Binding`)
- Modify: `crates/toolr-core/src/parser/build.rs` (`BuildError::UnsupportedCommands`, format fn, check after
  name conflicts)
- Create: `crates/toolr-py/python/toolr/testing/_shapes.py` (`_unsupported_commands`, `_format_unsupported_commands`)
- Modify: `crates/toolr-py/python/toolr/testing/_discovery.py` (`collected_command_groups` raises it first)
- Create: `tests/e2e/test_parser_tester_parity.py`, `tests/e2e/parity_corpus/<case>/*.py`
- Test: `build.rs` tests, `tests/registry/test_integration.py`

**Interfaces:**

```text
unsupported command declarations ({count}):
  - {module}::{function}: `@command` must decorate a top-level `def`, which toolr reads without running it.
  - {module}::{function}: `group=` must be a string literal.
  - {module}::{function}: the command name must be a string literal.
```

**Steps:**

- [ ] **Step 1: Rust tests.** Scratch `c` (command in a nested `def`), a command under a module-level `if`, a method
      in a class, scratch `d` (`group=NAME`) and `@command(NAME, group="grp")`, each asserting the full message. A
      nested `@cli.command` where `cli` isn't a group isn't reported.
- [ ] **Step 2: Python tests.** The same fixtures through `CommandsTester` → `ValueError` with the same text. A
      command defined in a test body under an active tester is not reported.
- [ ] **Step 3: Run** both → FAIL.
- [ ] **Step 4: Implement Rust.** Recurse into every compound statement's body (`ruff_python_ast::statement_visitor`)
      except the top-level `def` itself, reporting recognised command decorators. In `parse_*_command`, a present
      `group=`/name that isn't a string literal sets the flag. Report in source order per file.
- [ ] **Step 5: Implement Python.** For each registered, pending or duplicate function with `co_filename` under
      `search_path/tools`: `ast.parse` the file once (cache by path), find the def by `name` and `co_firstlineno`
      (`min(decorator lines, def line)`); not in `module.body` → nested line; a decorator `Call` whose `group=`
      keyword, `name=` keyword or first positional isn't a `str` `ast.Constant` → the matching line. Order as in
      Task 2.
- [ ] **Step 6: Corpus.** One parametrised test over `parity_corpus/*`, copying each case into `make_tools_project`.
      `ok_*`: `--help` exits 0; read the manifest the build wrote (confirm its path on the first run) and compare
      `{group: sorted(names)}` with the tester's `{_group_path(k): sorted(g.get_commands())}`. `err_*`: `--help`
      fails, the tester raises, `f"toolr: {exc}\n" in stderr`. Cases: `ok_plain`, `ok_toolr_attribute` (`a`),
      `ok_toolr_module_alias`, `ok_aliased_import` (`b`), `ok_cross_file_binding`, `err_nested_def` (`c`),
      `err_nested_if`, `err_nonliteral_group` (`d`), `err_nonliteral_name`, `err_dup_one_module` (`e`),
      `err_dup_two_modules` (`f`), `err_suggestion_tie` (`ab`, `ca`, `ac`; expects `ab`).
- [ ] **Step 7: Run** `cargo test -p toolr-core parser::` and `uv run pytest tests/registry tests/e2e` → PASS.
- [ ] **Step 8: Commit.** `fix(parser)!: reject nested and non-literal command declarations`, then
      `test(e2e): differential parser/tester corpus`.

### Task 5: Docs, skill refs, release notes, verification, archive

**Files:**

- Modify: `crates/toolr-py/python/toolr/_decorators.py` (`command` docstring), `docs/writing-commands/testing.md`,
  `docs/writing-commands/groups.md`, `docs/writing-commands/across-files.md`,
  `skills/toolr-command-authoring/references/*.md` (generated), `skills/toolr-command-authoring/SKILL.md` if needed

**Steps:**

- [ ] **Step 1:** Rewrite the "only one shape counts" paragraph in `command` and `testing.md:143-146`: the supported
      forms, the three rejected shapes and the duplicate rule, with the shared errors. One sentence in `groups.md` and
      `across-files.md`. Check `SKILL.md` prose by hand.
- [ ] **Step 2:** `cargo xtask build-skill-refs`; commit the regenerated references.
- [ ] **Step 3:** `toolr release-notes add --slug parser-tester-parity-breaking --text "**Breaking:** …"` (duplicates,
      nested and non-literal declarations now fail `toolr` and `CommandsTester`, with the fix for each) and
      `--slug parser-tester-parity --text "…"` (`@toolr.command`, `import toolr as t`, aliased `command` now register;
      equally close group suggestions are stable). Commit `docs(release-notes): …`.
- [ ] **Step 4:** `mise run test` (poll every 30-60 s), `prek run --all-files`, `uv run mkdocs build --strict`,
      `cargo llvm-cov …` then `cargo xtask patch-coverage --base main` → green, ≥99%.
- [ ] **Step 5:** `git mv` both spec files to `specs/archive/2026/`, fix the `Spec:`/`Plan:` links, set the design's
      status to implemented. Commit `docs(specs): archive parser and tester parity design and plan`.
