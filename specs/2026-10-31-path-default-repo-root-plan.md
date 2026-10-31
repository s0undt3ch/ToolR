# Path Default Repo Root Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A relative path default resolves from the repo root, so `--help` and the command work from any
subdirectory (F-11).

**Architecture:** For a path arg with a relative literal default, clap keeps the literal but parses it unchecked.
`execute_build` resolves the raw value afterwards, against the cwd when typed and against the repo root when
defaulted, reusing the same resolve-and-check code as `path_parser`.

**Tech Stack:** Rust, clap 4, `assert_cmd`.

**Spec:** [`specs/2026-10-31-path-default-repo-root-design.md`](./2026-10-31-path-default-repo-root-design.md)

## Global Constraints

- Typed values keep resolving against the cwd. Only defaults move to the repo root.
- `--help` never touches the filesystem for a default, and keeps printing the literal
  (`[default: tools/pyproject.toml]`).
- Bare `Path` (form `AsTyped`) is out of scope: it already resolves from the repo root in Python.
- No schema change.
- Conventional Commits; no `Co-Authored-By`. Stage by path; never `git add -A`. `rustfmt <file>` on touched files only.

## Review Focus

1. **Typed value from a subdirectory.** `--cfg x.toml` from `sub/dir` must resolve to `sub/dir/x.toml`, not the
   repo root's. Task 2, `typed_value_for_deferred_arg_resolves_against_cwd`.
2. **No false relative-path note.** A typed relative value on a deferred arg is a relative `PathBuf` in the matches;
   `relative_path_warning` must not fire for it. Task 2, `relative_path_warning_ignores_deferred_args`.
3. **Default that doesn't exist at the repo root either.** Must fail with exit 2 and name the repo root, not crash.
   Task 3, `missing_relative_default_errors_naming_repo_root`.
4. **`DirectoryPath = "."`** from a subdirectory resolves to the repo root. Task 2,
   `dot_directory_default_resolves_to_repo_root`.
5. **Dispatched leaves.** `pack_child_args` goes through the same path. Task 2,
   `pack_child_args_resolves_deferred_default_from_repo_root`.

---

### Task 1: Resolver and deferral predicate in `value_parsers.rs`

**Files:**

- Modify: `crates/toolr/src/value_parsers.rs` (`path_parser`, `absolutise`, new fns)
- Test: `crates/toolr/src/value_parsers.rs` `mod tests`

**Interfaces:**

- Produces: `pub fn defers_path_resolution(arg: &toolr_core::manifest::Argument) -> bool`

- Produces: `pub fn resolve_path(ty: &SupportedType, raw: &Path, base: &Path) -> Result<PathBuf, String>`. `ty` may
  be `Optional`-wrapped. The error text is today's (`path does not exist: <raw>`, `path is not a regular file: <raw>`,
  …), naming `raw` as given.

- [ ] **Step 1: Write the tests:**
    - `resolve_path_uses_the_given_base`: `base/tools/a.toml` exists, cwd elsewhere →
      `resolve_path(&FilePath, "tools/a.toml", base)` is `Ok(canonical base/tools/a.toml)`.
    - `resolve_path_errors_name_the_raw_value`: missing file →
      `Err("path does not exist: tools/missing.toml")`.
    - `resolve_path_absolute_form_joins_base`: `AbsolutePath`, `"build/out"` → `Ok(base/build/out)`, no disk check.
    - `defers_path_resolution_cases`: true for `Optional` + `FilePath` + `"tools/pyproject.toml"`, for
      `Optional(DirectoryPath)` + `"."` and for `NewPath` + `"out"`. False for bare `Path`, an absolute default, `""`,
      `"<expr>"`, a non-path type, a `Positional` kind and no default.

- [ ] **Step 2: Run** `cargo test -p toolr --lib value_parsers` → the new tests fail to compile.

- [ ] **Step 3: Implement.** Move the `path_parser` closure body into `resolve_path`. `AsTyped` returns `raw`
      unchanged; `Absolute` joins a relative `raw` onto `base`; `Canonical` checks existence of and canonicalises
      `base.join(raw)`. `path_parser` becomes cwd lookup plus `resolve_path`. `absolutise` takes `base`.

- [ ] **Step 4: Run** `cargo test -p toolr --lib value_parsers` → PASS, along with the existing tests.

- [ ] **Step 5: Commit.** `refactor(cli): resolve path values against an explicit base`

### Task 2: Defer resolution of relative path defaults

**Files:**

- Modify: `crates/toolr/src/cli.rs` (`build_user_command`, the `ArgumentKind::Optional` branch and the
  value-parser block after it)
- Modify: `crates/toolr/src/execute_build.rs` (`build_spec`, `build_dispatch_spec`, `pack_child_args`,
  `extract_value`, `extract_scalar`, `arg_has_relative_cli_path`)
- Modify: `crates/toolr/src/dispatch.rs` (the `build_spec` / `build_dispatch_spec` / `pack_child_args` call sites)
- Test: `crates/toolr/src/cli.rs` `mod tests`, `crates/toolr/src/execute_build.rs` `mod tests`

**Interfaces:**

- Consumes: `defers_path_resolution`, `resolve_path` (Task 1).

- Produces: `pub(crate) struct PathBases<'a> { pub cwd: &'a Path, pub repo_root: &'a Path }`.

- Produces: `build_spec(cmd, matches, bases: &PathBases, output_opts) -> anyhow::Result<ExecutionSpec>`,
  `build_dispatch_spec(parent, parent_matches, packed, bases: &PathBases, output_opts) -> anyhow::Result<ExecutionSpec>`,
  `pack_child_args(cmd, matches, bases: &PathBases) -> anyhow::Result<PackedChild>`. `dispatch` builds `PathBases` from
  `std::env::current_dir()` and its `repo_root`.

- [ ] **Step 1: Write the tests.** Build the clap command with `build_user_command`, parse with
      `try_get_matches_from`, and pass `PathBases` pointing at temp dirs. Never `set_current_dir`.
    - cli.rs `relative_path_default_parses_without_touching_disk`: `Optional` arg `cfg`, `FilePath`, default
      `"tools/pyproject.toml"`, nothing on disk → `try_get_matches_from(["hello"])` is `Ok`, and `get_default_values`
      is `["tools/pyproject.toml"]`.
    - execute_build.rs `default_resolves_against_repo_root`: the same arg; `repo/tools/pyproject.toml` exists;
      `cwd = repo/sub/dir` → `args["cfg"]` is the canonical `repo/tools/pyproject.toml`.
    - `typed_value_for_deferred_arg_resolves_against_cwd`: `cwd/x.toml` and `repo/x.toml` both exist; `--cfg x.toml` →
      `args["cfg"]` is `cwd/x.toml`.
    - `typed_missing_value_for_deferred_arg_errors_like_clap`: `--cfg nope.toml` → error contains
      `invalid value 'nope.toml' for '--cfg': path does not exist: nope.toml`.
    - `dot_directory_default_resolves_to_repo_root`: `DirectoryPath`, default `"."` → `args["dir"]` is canonical
      `repo`.
    - `absolute_path_default_joins_repo_root`: `AbsolutePath`, default `"build/out"` → `repo/build/out`.
    - `relative_path_warning_ignores_deferred_args`: `--cfg x.toml` typed, cwd ≠ repo root →
      `relative_path_warning` is `None`.
    - `pack_child_args_resolves_deferred_default_from_repo_root`: as `default_resolves_against_repo_root`, through
      `pack_child_args`.
    - Update existing `build_spec(` calls in `execute_build.rs` tests to the new signature and `.unwrap()`.

- [ ] **Step 2: Run** `cargo test -p toolr --lib cli:: execute_build::` → the new tests fail.

- [ ] **Step 3: Implement.** In `build_user_command`, after `apply_value_parser`, if
      `defers_path_resolution(arg)` set `a = a.value_parser(clap::value_parser!(PathBuf))`. In `extract_scalar`, for
      a deferred arg read the `PathBuf` and pick the base from `matches.value_source(name)`, then call `resolve_path`.
      Map errors to the two messages in the spec. Propagate `Result` up to `dispatch`, which already returns
      `anyhow::Result`. `arg_has_relative_cli_path` returns `false` for a deferred arg.

- [ ] **Step 4: Run** `cargo test -p toolr --lib` → PASS.

- [ ] **Step 5: Commit.** `fix(cli): resolve relative path defaults from the repo root`

### Task 3: Through the binary, from a subdirectory

**Files:**

- Create: `crates/toolr/tests/path_defaults.rs`

**Interfaces:** none.

The fixture uses the reported shape exactly: a real `tools/pyproject.toml` and a `tools/demo.py` with
`def hello(ctx: Context, *, cfg: FilePath = "tools/pyproject.toml") -> None:  # type: ignore[assignment]`. Build it
like `bool_flags.rs::project_with`, so the static manifest is built in Rust and `--help` needs no Python.

- [ ] **Step 1: Write the tests:**
    - `help_from_subdirectory_shows_relative_path_default`: `current_dir(root/sub/dir)`, `demo hello --help` →
      exit 0, stdout contains `[default: tools/pyproject.toml]`, stderr doesn't contain `path does not exist`.
    - `missing_relative_default_errors_naming_repo_root`: default `"tools/missing.toml"`, run `demo hello` from
      `sub/dir` → exit 2, stderr contains `default 'tools/missing.toml'` and `resolved from the repo root`. This fails
      before the runner starts, so no Python is needed.
    - `typed_missing_value_from_subdirectory_still_fails_at_parse`: `demo hello --cfg nope.toml` from `sub/dir` →
      exit 2, stderr contains `path does not exist: nope.toml`.

- [ ] **Step 2: Run** `cargo test -p toolr --test path_defaults` → PASS (Task 2 is in). Check that it fails against
      the parent commit: `git stash` the Task 2 code, then `help_from_subdirectory_shows_relative_path_default` fails
      with `path does not exist`. Restore it.

- [ ] **Step 3: Commit.** `test(cli): relative path defaults from a subdirectory`

### Task 4: Docs, release notes, verification, archive

**Files:**

- Modify: `docs/writing-commands/arguments.md` (**Limits**, the string-default bullet)
- Modify: `crates/toolr-core/src/parser/types/supported.rs` (`AbsolutePath` `validated_by`)
- Regenerate: `docs/writing-commands/files/supported-types.md`, `skills/toolr-command-authoring/references/`
- Modify: `UNRELEASED.md`
- [ ] **Step 1:** In the **Limits** bullet replace "A relative string default resolves against the working
      directory, not the repo root." with "A relative string default resolves from the repo root, like the command
      itself; a value typed on the command line resolves from the current directory." Set the `AbsolutePath`
      `validated_by` to `clap (absolutise vs cwd; defaults vs repo root)`. Run `cargo xtask build-skill-refs`, then
      `cargo xtask build-skill-refs --check` → clean.
- [ ] **Step 2:** Add the spec's `UNRELEASED.md` text verbatim. Commit
      `docs(cli): relative path defaults resolve from the repo root`.
- [ ] **Step 3:** `mise run test`, polling every 30-60 s → green. `prek run --all-files` → green.
      `uv run mkdocs build --strict` → green.
- [ ] **Step 4:**
      `git mv specs/2026-10-31-path-default-repo-root-design.md specs/2026-10-31-path-default-repo-root-plan.md specs/archive/2026/`;
      fix the plan's `Spec:` link and set the design's status to implemented. Commit
      `docs(specs): archive path default repo root design and plan`.
