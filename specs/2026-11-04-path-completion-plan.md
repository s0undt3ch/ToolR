# Path argument completion: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Path-typed arguments, user and built-in, tab-complete files or directories in bash, zsh
and fish (audit F-36).

**Architecture:** A core `SupportedType::path_completion()` table feeds both clap's `ValueHint`
and a new `complete::complete()` engine entry point. `toolr __complete` signals "complete paths"
with exit 10 (paths) or 11 (directories) and empty stdout. Each script maps that to native file
completion. Old scripts treat a non-zero exit as no candidates, which is today's behaviour.

**Tech Stack:** Rust 2024, clap 4 `ValueHint`, bash `compgen`, zsh `_files`, fish
`__fish_complete_path`, assert_cmd.

**Spec:** `specs/2026-11-04-path-completion-design.md`

**Decisions (2026-11-04):** `NewPath` completes directories only (exit 11). `ExecutablePath`
offers every file. Bash 4+ uses `compopt -o nospace` for directory candidates; bash 3.2 keeps
the trailing space. `--opt=value` stays out of scope. See the spec's "Decisions".

## Global Constraints

- Branch `feat/path-completion`, off `main`. Worktree `.worktrees/path-completion`.
- Stage files by path. Never `git add -A`. Never stage `audit/`.
- Conventional Commits. No Co-Authored-By trailer. British English.
- Format only the files you touch (`rustfmt --edition 2024 <file>`). Never `cargo fmt` a whole crate.
- 99% patch coverage on changed lines, `#[cfg(test)]` code in `src/` included. Use real tests, not
  mocks.
- No manifest `SCHEMA_VERSION` bump and no runner-spec bump: `resolved_type` already carries the
  type.
- Scripts never `eval` and never run a candidate or file name. Every file name goes through the
  same quoting as candidate values.

## Review Focus

- Old bash, zsh and fish scripts against the new binary on a path slot: no candidates, no error,
  nothing inserted.
- macOS `/bin/bash` 3.2 (no `compopt`): file names quoted with `%q`, directories get `/`, and a
  typed leading `~` stays unescaped so it still expands.
- A file named `$(touch X)` or containing a space completes as one inert word in every shell.
- `allowed_values` on a path-typed argument wins over file completion.
- `--<TAB>` after a path argument still lists flags, and a `list[Path]` variadic keeps completing
  paths past its first value.

---

### Task 1: Path hint table in toolr-core

**Files:**

- Modify: `crates/toolr-core/src/parser/types/supported.rs` (enum and method next to `is_path`)
- Modify: `crates/toolr-core/src/parser/types/mod.rs` (re-export)
- Modify: `crates/toolr/src/value_parsers.rs` (`path_hint`)
- Modify: `CONTRIBUTING.md` ("Adding a supported type", compiler-enforced list)

**Interfaces:**

- Produces: `pub enum PathCompletion { Any, File, Directory, Executable }` (`Debug, Clone, Copy,
  PartialEq, Eq`) and `pub fn path_completion(&self) -> Option<PathCompletion>` on `SupportedType`.
- [ ] **Step 1: Write the failing tests** in `supported.rs`'s test module:
    - `path_completion_maps_every_path_type`: one assertion per path variant, following the
      design's table.
    - `path_completion_unwraps_optional_and_list`: `Optional(FilePath)` gives `File` and
      `List(DirectoryPath)` gives `Directory`.
    - `path_completion_is_none_for_non_path_types`: `Str`, `Literal`, `Tuple([Path])`.
- [ ] **Step 2:** Run `cargo test -p toolr-core path_completion`. Expected: FAIL to compile.
- [ ] **Step 3: Implement** with an exhaustive `match` (no `_ =>`). `Optional` and `List` recurse.
      Rewrite `value_parsers::path_hint` to map `path_completion()` onto `ValueHint` (`Any` to
      `AnyPath`, `File` to `FilePath`, `Directory` to `DirPath`, `Executable` to
      `ExecutablePath`). Leave `PathCheck` for validation only. Existing `value_parsers` tests pin
      the hints; only `NewPath` moves, from `AnyPath` to `DirPath` (decision 3).
- [ ] **Step 4:** Add a step to CONTRIBUTING's compiler-enforced list:
      "`SupportedType::path_completion()`: say how TAB completes it."
- [ ] **Step 5:** Run `cargo test -p toolr-core parser::types` and
      `cargo test -p toolr --bin toolr value_parsers`. Expected: PASS.
- [ ] **Step 6: Commit** `feat(types): one path completion hint per supported type` with a
      `Release-Notes: skip` trailer.

### Task 2: Engine entry point

**Files:**

- Modify: `crates/toolr-core/src/complete/engine.rs`
- Modify: `crates/toolr-core/src/complete/mod.rs` (re-export `complete`, `Completion`)
- Test: `crates/toolr-core/src/complete/tests.rs`

**Interfaces:**

- Consumes: `SupportedType::path_completion` from Task 1.
- Produces: `pub enum Completion { Values(Vec<Candidate>), Paths, Directories }` and
  `pub fn complete(manifest: &Manifest, tokens: &[String]) -> Completion`.
- [ ] **Step 1: Write the failing tests** with a manifest helper holding one command with
      `cfg: Optional(FilePath)` (flag), `out: DirectoryPath` (flag), `exe: ExecutablePath` (flag),
      `src: Path` (positional), `rest: List(Path)` (var-positional) and `mode: Literal`:
    - `--cfg ""` gives `Paths`. `--out ""` gives `Directories`. `--exe ""` gives `Paths`.
    - `""` in the `src` slot gives `Paths`. `a b ""` (in `rest`) gives `Paths`.
    - `--mode ""` gives `Values` of the literals.
    - `--` after a path argument gives `Values` holding the flag names.
    - A path-typed argument with non-empty `allowed_values` gives `Values`.
    - A dispatcher child's path flag gives `Paths`.
- [ ] **Step 2:** Run `cargo test -p toolr-core complete::`. Expected: FAIL to compile.
- [ ] **Step 3: Implement** `complete`. Call `classify` once. For `FlagValue` or `Positional`
      whose `allowed_values` is empty, map
      `resolved_type.as_ref().and_then(SupportedType::path_completion)` to `Paths`
      (`Any`, `File`, `Executable`) or `Directories`. Otherwise return `Values` built the way
      `serve_completions` builds them. Share that code through a private helper that takes the
      `Slot`, so `serve_completions` and `complete` can't drift and nothing gets classified
      twice.
- [ ] **Step 4:** Run `cargo test -p toolr-core complete::`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(complete): report path slots from the completion engine` with a
      `Release-Notes: skip` trailer.

### Task 3: `__complete` exit codes and built-in hints

**Files:**

- Modify: `crates/toolr/src/dispatch.rs` (`run_complete`)
- Modify: `crates/toolr/src/cli.rs` (`--source-dir` and `--output` hints)
- Modify: `crates/toolr/src/builtin_completions.rs` (`derive_argument`)
- Test: `crates/toolr/tests/complete_paths.rs` (new) and the `builtin_completions` test module

**Interfaces:**

- Consumes: `toolr_core::complete::{complete, Completion}` from Task 2.
- Produces: exit codes `10` (paths) and `11` (directories), both with empty stdout. Name them as
  constants in `dispatch.rs`.
- [ ] **Step 1: Write the failing tests.**
    - `complete_paths.rs`: a project with `tools/demo.py` that matches the spec's repro command
      exactly (`src: pathlib.Path`, `cfg: FilePath | None`, `out: DirectoryPath | None`,
      `new: NewPath | None`, `mode: Literal["a", "b"]`). Assert that `--cfg ""` and `""` exit
      10, `--out ""` and `--new ""` exit 11, all with empty stdout, and that `--mode ""` exits 0
      with `a` and `b`. Built-ins: `project trust ""` exits 10, and
      `self build-manifest --source-dir ""` exits 11. `--output ""` exits 10.
    - `builtin_completions`: `derive_argument` sets `resolved_type` from each `ValueHint`, and a
      hint-less option keeps `None`.
- [ ] **Step 2:** Run `cargo test -p toolr --test complete_paths`. Expected: FAIL.
- [ ] **Step 3: Implement.** `run_complete` calls `complete`. `Values` prints as today and exits 0.
      `Paths` and `Directories` print nothing and exit with their code. Leave
      `merge_root_flags_at_top_level` on the `Values` path. In `cli.rs`, add
      `.value_hint(ValueHint::DirPath)` to `--source-dir` and `.value_hint(ValueHint::FilePath)`
      to `--output`. In `derive_argument`, map `get_value_hint()` (`AnyPath` to `Path`, `FilePath`
      to `FilePath`, `DirPath` to `DirectoryPath`, `ExecutablePath` to `ExecutablePath`, any other
      hint to `None`).
- [ ] **Step 4:** Run `cargo test -p toolr --test complete_paths --test cli_smoke` and
      `cargo test -p toolr --bin toolr builtin_completions`. Expected: PASS. Confirm that every
      existing `__complete` test still sees exit 0.
- [ ] **Step 5: Commit** `feat(cli): signal path slots from toolr __complete` with a
      `Release-Notes: skip` trailer. The note lands in Task 5.

### Task 4: Shell scripts

**Files:**

- Modify: `crates/toolr-core/src/complete/scripts/bash.sh`, `zsh.zsh`, `fish.fish`
- Test: `crates/toolr/tests/complete_shells.rs`

**Interfaces:**

- Consumes: exit codes 10 and 11 from Task 3.
- [ ] **Step 1: Write the failing tests** in `complete_shells.rs`. Add a `cfg: Path | None` and an
      `out: DirectoryPath | None` argument to the fixture's `lint` command. Then create
      `files/with space.txt`, `files/sub/` and `files/$(touch SENTINEL3)` under the fixture root.
    - bash, on each `bashes()` entry, with a variant of `run_bash` that takes the flag and the
      word: `--cfg files/` holds the `%q` forms of all three entries, with `files/sub/` ending in
      `/`. `SENTINEL3` doesn't exist. `--out files/` holds only `files/sub/`. A `~/` prefix comes
      back starting with `~/`, not `\~/`. The driver records `compopt` calls only where the
      bash has the builtin: `--out` requests `nospace` there, and bash 3.2 never calls it.
    - bash compatibility: run the script text from `main` (paste it into the test as a `const`)
      on `--cfg files/`. It returns rc 0 with an empty `COMPREPLY`.
    - fish: `complete -C "toolr ci lint --cfg files/"` lists `files/with space.txt` and
      `files/sub/`, and the sentinel stays absent. `--out files/` lists only `files/sub/`.
    - zsh (source inspection): the script calls `_files` and `_files -/`, and has no `eval`.
- [ ] **Step 2:** Run `cargo test -p toolr --test complete_shells`. Expected: the new tests FAIL.
- [ ] **Step 3: Implement.**
    - bash: capture `rc` without `|| return 0`. On 10 or 11, read `compgen -f -- "$raw_cur"`
      (or `-d`) with `while IFS= read -r`. Quote each name with `printf %q` and put the leading
      `~` back if `raw_cur` started with `~`. Add `/` when `[[ -d $name ]]`, after tilde
      expansion for the test. If bash has `compopt`, run `compopt -o nospace` when the only
      candidate is a directory. Bash 3.2 keeps the trailing space. Any
      other non-zero `rc` returns 0, as today.
    - zsh: run `toolr __complete` once into `lines`, then read `$?`. On 10 call `_files`, on 11
      call `_files -/`, and return their status.
    - fish: `set -l out (toolr __complete … 2>/dev/null)` and then
      `switch $status`. Case 10 runs `__fish_complete_path "$current"`, case 11 runs
      `__fish_complete_directories "$current"`, and case 0 prints `$out` line by line.
- [ ] **Step 4:** Run `cargo test -p toolr --test complete_shells`. Expected: PASS. Also run
      `shellcheck crates/toolr-core/src/complete/scripts/bash.sh`, or check that prek runs it.
- [ ] **Step 5:** Smoke-test by hand in an interactive zsh: `source <(toolr self completion print
      zsh)` in the scratch repo, then `toolr demo hello --cfg <TAB>` and `--out <TAB>`.
- [ ] **Step 6: Commit** `feat(complete): native file completion for path arguments in bash, zsh
      and fish`, with a `Release-Notes: skip` trailer.

### Task 5: Docs and release note

**Files:**

- Modify: `docs/cli.md` (`__complete` section), `docs/concepts.md` ("Tab completion"),
  `docs/writing-commands/arguments.md` (path types)
- Modify, if it mirrors that text: `skills/toolr-command-authoring/references/arguments.md`
- Create: a release note via `toolr release-notes add`
- [ ] **Step 1:** Add the exit-code table to `docs/cli.md`. Add one sentence to concepts saying
      that path arguments complete from the file system, and the reinstall hint. In
      `arguments.md`, say which path types complete directories only.
- [ ] **Step 2:** Run `toolr release-notes add --text "…"` with the design's release-note text.
- [ ] **Step 3:** Run `cargo xtask build-skill-refs --check`, `prek run --all-files` and
      `mkdocs build --strict`. Expected: clean. If the doc-snippets hook fails for a missing tools
      venv, run `toolr project venv sync -q` and retry.
- [ ] **Step 4: Commit** the docs as `docs(complete): document path argument completion` and the
      note as `docs(release-notes): path arguments tab-complete files`.

### Task 6: Verify, then archive the specs

- [ ] **Step 1:** Run `mise run test`, polling the output every 30–60 seconds.
- [ ] **Step 2:** Run the CI coverage command from CLAUDE.md, then
      `cargo xtask patch-coverage --base main`. Expected: 99% or more. Close any gap with a real
      test.
- [ ] **Step 3:** `git mv specs/2026-11-04-path-completion-design.md specs/archive/2026/` and the
      same for the plan. Commit `docs(specs): archive path completion spec and plan` with a
      `Release-Notes: skip` trailer. This is the final commit before the PR.
