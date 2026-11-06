# Unsupported command_group binding forms: implementation plan

> **For agentic workers:** use superpowers:subagent-driven-development or superpowers:executing-plans.
> Steps use checkbox (`- [ ]`) syntax.

**Goal:** a `command_group` call the static parser can't bind fails the build, and `CommandsTester`,
with "unsupported command declarations", instead of vanishing.

**Architecture:** `bound_group_call` (shape match plus `command_group` test) shared by the binding pass and a
detector, a new `UnsupportedCommandKind::GroupBinding`, and a Python mirror in `testing/_groups.py` and
`testing/_shapes.py`.

**Spec:** `specs/archive/2026/2026-11-06-group-binding-forms-design.md`

## Global Constraints

- Plain top-level `g = command_group(...)`, `g: T = ...` and bare `command_group(...)` build exactly
  as today.
- No manifest schema change; no `RUNNER_SCHEMA_VERSION` bump.
- The Rust and Python messages are byte-identical; the parity corpus proves it.
- Patch coverage at least 99% with real tests.
- Commits: Conventional Commits, no Co-Authored-By, `TOOLR_TRUST_ALL=1`, stage files by path.

## Review Focus

- `group_binding` and the detector use the one `bound_group_call` helper: no statement is both
  bound and reported, or neither.
- A call nested in a supported statement's arguments, and `command_group(...).command`.
- An alias that resolves to something that isn't toolr's `command_group` isn't reported.
- Source order on both sides, including calls in one statement (`d[a()] = b()`, comprehensions, `*`/keyword arguments).
- Only parameters shadow `command_group`; a local `def`, an assignment or a foreign alias is resolved by the import tables.
- Group problems come before command problems in both messages.

---

### Task 1: Rust detector

**Files:**

- Modify: `crates/toolr-core/src/parser/commands.rs` (`UnsupportedCommandKind::GroupBinding` and its `Display`;
  `ToolrNames::is_toolr_module` becomes `pub(super)`)
- Modify: `crates/toolr-core/src/parser/groups.rs` (`supported_group_call`, `bound_group_call`,
  `detect_unsupported_group_bindings`, `UnboundGroupCalls`, `NAME` param)
- Modify: `crates/toolr-core/src/parser/build.rs` (collect, chain with `concat`)
- Test: unit tests in `groups.rs` and `build.rs`

- [ ] Write the failing unit tests (bound shapes unreported; tuple, chained, walrus, compound bodies, function,
      class, lambda, comprehension, nested expression, aliased and attribute calls reported; calls that aren't toolr's
      and parameter shadowing not reported; name-only calls reported; keyword and non-literal names; source order;
      `bound_group_call` agrees with `group_binding`); run `cargo test -p toolr-core parser::groups`.
- [ ] Implement, then `rustfmt --edition 2024` on the touched files only.
- [ ] `build.rs` tests: the exact message for several calls, and groups before commands across modules.

### Task 2: Python mirror

**Files:**

- Modify: `crates/toolr-py/python/toolr/testing/_groups.py`, `_shapes.py`, `_discovery.py` (`Raises`)
- Test: `tests/testing/test_groups.py`

- [ ] Mirror each Rust piece (`_bound_group_call`, `_unbound_group_calls` with an explicit stack, `_is_toolr_module`,
      `_parameter_names`, `_group_call_label`, `_unsupported_group_bindings`) with "Mirrors" pointers both ways.
- [ ] Write the matching tests, including a 3000-term expression.
- [ ] `uv run ruff format` and `uv run ruff check` on the touched files only.

### Task 3: Parity corpus

- [ ] Add the cases listed in the design's Corpus section and the naming notes in the corpus `README.md`; see each
      `err_` case fail before Tasks 1 and 2 and pass after, and every existing case still pass.
- [ ] Extend the differential harness to compare each side's groups and commands per group, not just OK or ERR.

### Task 4: Docs, release note

- [ ] Doc comment on `detect_unsupported_group_bindings` as the canonical rule; repeat it in the `command` docstring,
      `docs/writing-commands/testing.md` and `skills/toolr-command-authoring/SKILL.md`; one pointer line in the
      `command_group` docstring; `cargo xtask build-skill-refs`.
- [ ] `toolr release-notes add --text "**Breaking:** …"`, trimmed to what changed and how to fix it.

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
