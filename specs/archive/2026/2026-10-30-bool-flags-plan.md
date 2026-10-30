# Boolean Flags Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reject every `bool` parameter shape except `name: bool = False` on native commands, and make aliases of `bool` behave as flags.

**Architecture:** A single check runs in the type resolver (`resolve_one`) right after an argument's type resolves. It promotes alias-of-`bool` keyword arguments from `Optional` to `Flag`, then validates the shape and pushes a `TypeResolutionError` with a new `UnsupportedType::BoolFlag` reason. The existing `BuildError::UnsupportedTypes` path reports every violation at once. No CLI, completion, manifest, runner or schema code changes.

**Tech Stack:** Rust (toolr-core parser), `assert_cmd` integration tests, mkdocs docs.

**Spec:** [`specs/2026-10-30-bool-flags-design.md`](./2026-10-30-bool-flags-design.md)

## Global Constraints

- Applies to native commands only. argparse-scanned sources (`crates/toolr-core/src/argparse/`) must not change behaviour.
- No change to `RUNNER_SCHEMA_VERSION`, Python `SCHEMA_VERSION`, the manifest schema version, `ArgumentKind` or `ArgMetadata`.
- Error text is fixed by the spec's "Error messages" table. Copy it verbatim.
- `<neg>` = `no_<name>`, unless `<name>` starts with `no_`, in which case strip the prefix. CLI spelling replaces `_` with `-`.
- Conventional Commits. No `Co-Authored-By` footer. Stage files by path; never `git add -A` (the repo has a local-only `audit/` dir).
- Format only touched Rust files (`rustfmt <file>`), never `cargo fmt -p`.

## Review Focus

1. **Alias of `bool` with `= False`** (`Verbose = Annotated[bool, arg(env="V")]`, `verbose: Verbose = False`) must build as `Flag`, not error and not stay value-taking. Owned by Task 1, test `bool_alias_with_false_default_becomes_flag`.
2. **`Annotated[bool, arg(...)] = True`** must be rejected the same as bare `bool = True`. Task 1, test `annotated_bool_true_default_rejected`.
3. **argparse `store_false` with `default=True`** must still build and keep its `Flag` kind and `"true"` default. Task 1, test `argparse_store_false_unaffected` (in the argparse scan tests).
4. **Several violations in one tree** must all be reported in one build error, not just the first. Task 1, test `all_bool_violations_reported_together`.
5. **`no_` prefix suggestion**: `no_cache: bool = True` must suggest `cache: bool = False`, not `no_no_cache`. Task 1, test `no_prefixed_true_default_suggests_positive_name`.

---

### Task 1: Bool flag rule in the type resolver

**Files:**
- Create: `crates/toolr-core/src/parser/types/bool_flag.rs`
- Modify: `crates/toolr-core/src/parser/types/mod.rs` (declare `mod bool_flag;`)
- Modify: `crates/toolr-core/src/parser/types/supported.rs` (`UnsupportedType` enum ~489-508, its `Display` ~540-560)
- Modify: `crates/toolr-core/src/parser/types/resolve.rs` (`resolve_one`, `Ok(ty)` branch ~145-164)
- Test: `crates/toolr-core/src/parser/build.rs` `mod tests` (reuse `type_errors_for`, `kw_module`, `TOOLR_IMPORTS`, ~1344, ~1730, ~1749)
- Test: argparse scan tests next to `crates/toolr-core/src/argparse/scan.rs` (Review Focus 3)

**Interfaces:**
- Produces: `pub enum BoolFlagError { DefaultTrue { suggested: String }, NoDefault, NotKeyword, NotPlainFlag }` in `bool_flag.rs`.
- Produces: `UnsupportedType::BoolFlag { argument: String, error: BoolFlagError }`. `argument` is the parameter name, so `Display` can render `<name>` in the messages.
- Produces: `pub(super) fn check_bool_flag(ty: &SupportedType, arg: &mut Argument) -> Result<(), UnsupportedType>` in `bool_flag.rs`.

- [ ] **Step 1: Write the failing tests** in `build.rs` `mod tests`, each using `type_errors_for(&[("tools/kw.py", &kw_module(TOOLR_IMPORTS, <prelude>, <signature>))])`, then matching on `errs[0].reason` and `errs[0].to_string()`:

| Test | prelude | signature | Assert |
|---|---|---|---|
| `bool_true_default_rejected` | `""` | `colour: bool = True` | reason is `BoolFlag { error: DefaultTrue { suggested: "no_colour" }, .. }`; message contains ``Write `no_colour: bool = False` (CLI: `--no-colour`)`` |
| `annotated_bool_true_default_rejected` | `""` | `colour: Annotated[bool, arg(env="C")] = True` | same reason as above |
| `no_prefixed_true_default_suggests_positive_name` | `""` | `no_cache: bool = True` | `suggested == "cache"`; message contains `` `cache: bool = False` `` |
| `kwonly_bool_without_default_rejected` | `""` | `*, verbose: bool` | `BoolFlag { error: NoDefault, .. }` |
| `positional_bool_rejected` | `""` | `flag: bool` | `NotKeyword` |
| `var_positional_bool_rejected` | `""` | `*flags: bool` | `NotKeyword` |
| `optional_bool_rejected` | `""` | `colour: bool \| None = None` | `NotPlainFlag` |
| `typing_optional_bool_rejected` | `""` | `colour: Optional[bool] = None` | `NotPlainFlag` |
| `list_of_bool_rejected` | `""` | `*, flags: list[bool] = []` | `NotPlainFlag` |
| `tuple_of_bool_rejected` | `""` | `*, flags: tuple[bool, bool] = (False, False)` | `NotPlainFlag` |
| `all_bool_violations_reported_together` | `""` | `flag: bool, *, colour: bool = True, verbose: bool` | `errs.len() == 3` |

Plus one success test, `bool_alias_with_false_default_becomes_flag`, using the existing successful-build pattern (`build_static_manifest(&tmp.path().join("tools")).unwrap()`, as at ~1320). Prelude `Verbose = Annotated[bool, arg(env="V")]`, signature `verbose: Verbose = False`. Assert the argument's `kind == ArgumentKind::Flag` and `default == Some("false".into())`.

Plus `plain_false_default_still_builds`: `verbose: bool = False, *, dry_run: bool = False` builds; both are `Flag`.

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p toolr-core --lib parser::build::tests::bool -- --nocapture` (and the other new names)
Expected: compile failure (no `BoolFlag` variant), then, once stubbed, assertion failures.

- [ ] **Step 3: Add `BoolFlagError` and `UnsupportedType::BoolFlag`**

Put `Display` for `BoolFlag` in `supported.rs`'s existing `match`, delegating to a `fn render(&self, argument: &str, f)` on `BoolFlagError`. The message text is the spec's "Error messages" table, verbatim. Hyphenate `suggested` for the `--…` spelling.

- [ ] **Step 4: Implement `check_bool_flag`**

Rules, in order:
1. If `ty` is `SupportedType::Bool` and `arg.kind == ArgumentKind::Optional`, set `arg.kind = ArgumentKind::Flag`. This is the alias case, where the syntactic classifier saw only a `Name`.
2. If `ty` is `Bool`:
   - `kind` `Positional` or `VarPositional` → `NotKeyword`;
   - `default` `None` → `NoDefault`;
   - `default` not `Some("false")` → `DefaultTrue { suggested }`;
   - otherwise `Ok`.
3. If `ty` contains `Bool` anywhere inside `Optional`, `List` or `Tuple` (recurse) → `NotPlainFlag`.
4. Otherwise `Ok`.

`suggested` comes from `arg.name`: strip a leading `no_` if present, otherwise prepend `no_`.

- [ ] **Step 5: Call it from `resolve_one`**

In the `Ok(ty)` branch, after the existing `Count`/`Repeated` promotions and before `arg.resolved_type = Some(ty)`, call `check_bool_flag(&ty, arg)`. On `Err(reason)`, push a `TypeResolutionError` built exactly like the existing `Err` arm (~165-171). Set `resolved_type` either way.

- [ ] **Step 6: Add the argparse guard test** (Review Focus 3)

In the existing argparse scan tests, scan a parser with `p.add_argument("--no-colour", dest="colour", action="store_false")`. Assert the argument's kind is `Flag`, its default is `"true"`, and the build succeeds. If an equivalent test already exists, cite it in the commit message instead of adding one.

- [ ] **Step 7: Run the toolr-core suite**

Run: `cargo test -p toolr-core`
Expected: all pass. If an existing test builds a manifest from a now-rejected shape, rewrite it to `name: bool = False`. Do not weaken the new tests.

- [ ] **Step 8: Commit**

```bash
rustfmt crates/toolr-core/src/parser/types/bool_flag.rs crates/toolr-core/src/parser/types/supported.rs crates/toolr-core/src/parser/types/resolve.rs crates/toolr-core/src/parser/types/mod.rs crates/toolr-core/src/parser/build.rs
git add <those files> <argparse test file if changed>
git commit -m "feat(parser)!: boolean parameters must be flags defaulting to False"
```

### Task 2: End-to-end check through the binary

**Files:**
- Test: `crates/toolr/tests/bool_flags.rs` (new; follow the fixture style in `crates/toolr/tests/freshness_dispatch.rs` ~282-360: write `tools/pyproject.toml` + `tools/<mod>.py` into a `TempDir`, run `Command::cargo_bin("toolr")`)

**Interfaces:**
- Consumes: Task 1's error text.

- [ ] **Step 1: Write the tests**
  - `bool_true_default_fails_help_with_rewrite`: `colour: bool = True`. `toolr --help` exits non-zero; stderr contains `invalid parameter declarations` and `` `no_colour: bool = False` ``.
  - `no_prefixed_flag_shows_in_help`: `no_colour: bool = False`. `toolr <group> <cmd> --help` succeeds; stdout contains `--no-colour`.
  - `bool_alias_is_a_flag_in_help`: alias `Verbose = Annotated[bool, arg(env="V")]`, `verbose: Verbose = False`. Help shows `--verbose` with no value placeholder (no `<VERBOSE>`).

- [ ] **Step 2: Run them**

Run: `cargo test -p toolr --test bool_flags`
Expected: PASS (Task 1 is already in).

- [ ] **Step 3: Commit**

```bash
git add crates/toolr/tests/bool_flags.rs
git commit -m "test(cli): cover boolean flag rules through the binary"
```

### Task 3: Docs, skill prose, release notes

**Files:**
- Modify: `docs/writing-commands/arguments.md` ("Boolean flags", ~121-127)
- Modify: `docs/writing-commands/files/flags-example.py` (add a `no_cache: bool = False` example)
- Modify: `skills/toolr-command-authoring/SKILL.md` (near the `loud: bool = False` example, ~88)
- Modify: `UNRELEASED.md`

- [ ] **Step 1: Rewrite the "Boolean flags" section** with the five points in the spec's "Documentation" section. Show the `if no_colour:` and `colour = not no_colour` idioms, and the `Literal["auto", "always", "never"]` alternative.

- [ ] **Step 2: Add one paragraph to the skill** stating the rule and the `no_<name>` rewrite. Check `skills/toolr-command-authoring/references/commands.md`; it's generated, so if it mentions `bool` defaults, regenerate it with `cargo xtask build-skill-refs`, don't hand-edit it.

- [ ] **Step 3: Add the `UNRELEASED.md` entry**, marked as breaking. List each rejected shape and its rewrite. Mention that alias-of-`bool` parameters are now flags (they used to take `true`/`false` values).

- [ ] **Step 4: Verify**

Run: `cargo xtask build-skill-refs --check && uv run mkdocs build --strict && prek run --all-files`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add docs/writing-commands/arguments.md docs/writing-commands/files/flags-example.py skills/toolr-command-authoring/SKILL.md UNRELEASED.md
git commit -m "docs(arguments): document boolean flag rules"
```

### Task 4: Full verification and spec archive

- [ ] **Step 1:** Run `mise run test`. Poll the output every 30-60 s; it can stall. Expected: green.
- [ ] **Step 2:** `git mv specs/2026-10-30-bool-flags-design.md specs/2026-10-30-bool-flags-plan.md specs/archive/2026/`, then fix the plan's relative `Spec:` link.
- [ ] **Step 3:** Commit: `docs(specs): archive bool flags design and plan`.
