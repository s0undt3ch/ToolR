# `--opt=value` completion: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** `--opt=<TAB>` completes the value (choices, files or directories) in bash, zsh and
fish, for user and built-in options (FU-15).

**Architecture:** The engine treats an in-progress `--name=partial` as the `FlagValue` slot of
`--name` with prefix `partial`, so values, path exit codes (10/11) and built-ins all reuse the
existing paths. Candidates stay bare. Each script splits the word at the first `=` and adds
the `--opt=` part the way its shell needs: bash trims to readline's word, zsh uses
`compset -P 1 '*='`, fish prints `$opt_prefix$value`.

**Tech Stack:** Rust 2024, bash `compgen`, zsh `compset`/`_files`, fish `__fish_complete_path`,
assert_cmd.

**Spec:** `specs/2026-11-04-opt-equals-completion-design.md`

**Decisions (proposed 2026-11-04):** one engine word shape (`--name=partial`); prefix added in
the scripts; short attached values and root flags out of scope. See the spec's "Decisions".

## Global Constraints

- Branch `feat/opt-equals-completion`, stacked on `fix/bash-completion-words` (FU-16, FU-17).
  Worktree `.worktrees/opt-equals-completion`. Patch coverage base is
  `fix/bash-completion-words`, not `main`.
- Stage files by path. Never `git add -A`. Never stage `audit/`.
- Conventional Commits. No Co-Authored-By trailer. British English.
- Format only the files you touch (`rustfmt --edition 2024 <file>`).
- No manifest or runner schema bump. The `__complete` exit codes are unchanged.
- Scripts never `eval` and never run a value or file name. bash stays errexit-, nounset- and
  pipefail-safe on `/bin/bash` 3.2.
- Build on FU-16's rebuilt bash word and readline-word trim. Don't add a second one. If FU-16
  landed a different shape, adapt Task 3 to it and note the change in the spec.

## Review Focus

- `--mode=x=` (a value containing `=`) splits at the first `=` in the engine and in all
  three scripts.
- bash 4+ word shape (`--mode`, `=`, `a`) reaches the engine as `--mode=a` via FU-16.
- Boolean and unknown `--name=` offer nothing, not flag names.
- Old scripts against the new binary offer nothing for `--opt=`, as today.

---

### Task 1: Engine attached value slot

**Files:**

- Modify: `crates/toolr-core/src/complete/engine.rs`
- Test: `crates/toolr-core/src/complete/tests.rs`

**Interfaces:**

- Produces: private `fn attached_value<'a>(command: &'a Command, word: &str) -> Option<Slot<'a>>`.
  `None` unless `word` starts with `--` and contains `=`. Otherwise `Some(FlagValue)` for a
  value-taking flag found by `find_flag`, and `Some(Slot::None)` for anything else.
- [ ] **Step 1: Write the failing tests** with a manifest holding `mode: Literal["alpha", "abc",
      "x=~root"]`, `cfg: Optional(FilePath)`, `out: DirectoryPath`, a boolean `verbose`, and
      `mode` aliased as `--style`:
    - `serve_completions`: `--mode=` gives all three bare values, `--mode=a` gives `abc` and
      `alpha`, `--mode=x=` gives `x=~root`, `--style=a` matches like `--mode=a`.
    - `--verbose=`, `--nope=` and `--=` give an empty list.
    - `complete`: `--cfg=` gives `Paths`, `--out=` gives `Directories`, `--mode=` gives
      `Values`.
    - A dispatcher's own `--flag=` and a grafted child's `--flag=` complete their values.
    - `--mode=alpha ""` on a command with one positional offers that positional's values.
- [ ] **Step 2:** Run `cargo test -p toolr-core complete::`. Expected: the new tests FAIL.
- [ ] **Step 3: Implement** `attached_value` with `split_once('=')`. Call it first in
      `classify_leaf_args` and in `classify_dispatcher`'s no-child branch, returning its slot
      when `Some`.
- [ ] **Step 4:** Run `cargo test -p toolr-core complete::`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(complete): complete the value of --opt=value` with a
      `Release-Notes: skip` trailer.

### Task 2: `__complete` coverage for user and built-in options

**Files:**

- Test: `crates/toolr/tests/complete_paths.rs`
- [ ] **Step 1: Write the tests** on the existing `demo` fixture: `--cfg=""` exits 10 and
      `--out=""` exits 11, both with empty stdout; `--mode=` exits 0 with bare `a` and `b`.
      Built-ins: `self build-manifest --output=` exits 10, `--source-dir=` exits 11,
      `project init --venv-location=c` prints `cache`.
- [ ] **Step 2:** Run `cargo test -p toolr --test complete_paths`. Expected: PASS with Task 1.
      If a built-in case fails, fix it in `builtin_completions.rs`, not in the test.
- [ ] **Step 3: Commit** `test(complete): cover --opt=value through __complete` with a
      `Release-Notes: skip` trailer.

### Task 3: Shell scripts

**Files:**

- Modify: `crates/toolr-core/src/complete/scripts/bash.sh`, `zsh.zsh`, `fish.fish`
- Test: `crates/toolr/tests/complete_shells.rs`

**Interfaces:**

- Consumes: bare candidates and exit 10/11 for `--name=partial` from Task 1; FU-16's rebuilt
  word and readline-word trim in `bash.sh`.
- [ ] **Step 1: Write the failing tests.**
    - bash driver: pass readline's word as `$2` to `_toolr_complete`, and add a variant with
      bash 4's split words (`toolr ci lint --mode = dr`, `cword` on `dr`, `$2` = `dr`). On every
      `bashes()` entry and both shapes: `--mode=` offers every choice `%q`-quoted;
      `--mode=dr` filters; `--mode=x=` (readline word `~r` after `x=~r`) inserts `\~root`;
      `--cfg=files/` gives the quoted names with `files/sub/`; `--out=files/` gives only
      `files/sub/`; `--cfg=~/` keeps the bare `~`; no sentinel exists; the errexit test gains
      a `--mode=` case. Old-script test: `--mode=dr` offers nothing.
    - zsh driver: stub `compset() { print -r -- "COMPSET:$*"; }`. `--mode=a` records
      `COMPSET:-P 1 *=` before `COMPADD:`; `--cfg=files/` records it before `FILES:`;
      `--out=` before `FILES:-/`; `--mode a` records no `COMPSET`.
    - fish: `complete -C "toolr ci lint --mode=dr"` lists `--mode=dry run` and
      `--mode=dry walk`; `--cfg=files/` lists `--cfg=files/with space.txt` and
      `--cfg=files/sub/`; `--out=files/` lists only `--out=files/sub/`; sentinel absent.
- [ ] **Step 2:** Run `cargo test -p toolr --test complete_shells`. Expected: new tests FAIL.
- [ ] **Step 3: Implement.**
    - bash: when `raw_cur` matches `--?*=*`, set `opt_prefix=${raw_cur%%=*}=` and
      `val_cur=${raw_cur#*=}`; else both empty and `val_cur=$raw_cur`. Filter candidates on
      `val_cur`, build `opt_prefix$value`, apply FU-16's trim, then `_toolr_quote`. Pass
      `opt_prefix` and `val_cur` to `_toolr_complete_paths`, which runs `compgen` on
      `val_cur` and puts each name through the same prefix-then-trim step before quoting.
      Keep `local` declarations and `|| :` guards so `set -eu` callers stay safe.
    - zsh: `[[ $cur == --?*=* ]] && compset -P 1 '*='` after reading the exit code and before
      `compadd`, `_files` or `_files -/`.
    - fish: `set -l opt_prefix (string match -r -- '^--[^=]+=' $current)`; case 0 prints
      `"$opt_prefix$line"`. Cases 10 and 11 stay as they are.
- [ ] **Step 4:** Run `cargo test -p toolr --test complete_shells` and
      `shellcheck crates/toolr-core/src/complete/scripts/bash.sh`. Expected: PASS.
- [ ] **Step 5:** Smoke-test by hand in an interactive zsh and `/bin/bash` in the scratch repo:
      `toolr ci lint --mode=<TAB>`, `--cfg=<TAB>`, `--out=<TAB>`.
- [ ] **Step 6: Commit** `feat(complete): complete --opt=value in bash, zsh and fish` with a
      `Release-Notes: skip` trailer.

### Task 4: Docs and release note

**Files:**

- Modify: `docs/cli.md` (`__complete`), `docs/concepts.md` ("Tab completion")
- Create: a release note via `toolr release-notes add`
- [ ] **Step 1:** In `docs/cli.md`, add one sentence: a `--name=partial` word completes as the
      value of `--name`, and candidates are bare values. In `docs/concepts.md`, say
      `--opt=value` completes like `--opt value`.
- [ ] **Step 2:** `toolr release-notes add --text "…"` with the spec's text.
- [ ] **Step 3:** Run `prek run --all-files` and `mkdocs build --strict`. If the doc-snippets
      hook fails for a missing tools venv, run `toolr project venv sync -q` and retry.
- [ ] **Step 4: Commit** `docs(complete): document --opt=value completion` and
      `docs(release-notes): --opt=value tab-completes its value`.

### Task 5: Verify, then archive the specs

- [ ] **Step 1:** Run `mise run test`, polling the output every 30–60 seconds.
- [ ] **Step 2:** Run the CI coverage command from CLAUDE.md, then
      `cargo xtask patch-coverage --base fix/bash-completion-words`. Expected: 99% or more.
- [ ] **Step 3:** `git mv` both spec files to `specs/archive/2026/` and commit
      `docs(specs): archive FU-15 --opt=value completion spec and plan` with a
      `Release-Notes: skip` trailer. This is the final commit before the PR.
