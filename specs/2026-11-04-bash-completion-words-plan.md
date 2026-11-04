# Bash completion word handling: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bash completion handles words containing `:` or `=` (FU-16) and a user `IFS` without a
space (FU-17), on bash 3.2 and 5, without needing bash-completion.

**Architecture:** `_toolr_complete` rebuilds its words from `COMP_WORDS`, `COMP_LINE` and
`COMP_POINT` instead of calling `_init_completion`. It records `raw_prefix`, the part of the word
readline keeps, and quotes only the remainder of each reply. A `local IFS` pins word splitting.

**Tech Stack:** bash 3.2 and 5, assert_cmd, `common::run_on_pty`.

**Spec:** `specs/2026-11-04-bash-completion-words-design.md`

**Decisions:** see the spec's "Decisions". This plan assumes the recommended answers. If the user
rejects the pty test, skip Task 3.

## Global Constraints

- Branch `fix/bash-completion-words`, off `main`. Worktree `.worktrees/bash-completion-words`.
- Stage files by path. Never `git add -A`. Never stage `audit/`.
- Conventional Commits. No Co-Authored-By trailer. British English.
- No `eval` on any typed word, value or file name. Every reply still goes through `_toolr_quote`.
- Bash 3.2-safe: no `compopt` without the `type` guard, no associative arrays, no namerefs, no
  `${var@Q}`.
- `set -e`- and `set -u`-safe: `${COMP_WORDBREAKS-}`, `|| :` on commands that may fail.
- FU-15 (`--opt=value`) stays out. Keep `cur`, `raw_cur` and `raw_prefix` as separate variables
  so FU-15 can split at `=` without reworking them.

## Review Focus

- A reply never repeats text readline keeps, on either bash, for values and for file names.
- An escaped break (`a\:b`) or quote does not count as a break.
- A typed `~` still expands (`~/files/host:p`).
- Bash 3.2 under `IFS=$'\n'` with `set -eu` completes.
- `OLD_BASH_SCRIPT` still offers nothing on a path slot.

---

### Task 1: Driver and failing tests

**Files:**

- Modify: `crates/toolr/tests/complete_shells.rs`

**Interfaces:**

- Produces: `enum Split { Whole, AtBreaks }` and a `run_bash_script` that takes the typed words and
  a `Split`, and sets `COMP_LINE`, `COMP_POINT` (end of line), `COMP_WORDS` and `COMP_CWORD`.
  `AtBreaks` turns every `:` and `=` in a word into its own `COMP_WORDS` entry, the way bash 5
  does (`a:b` becomes `a`, `:`, `b`; `a:` becomes `a`, `:`). `COMP_LINE` joins the typed words
  with single spaces. The driver defines the `_init_completion` stub only when a `legacy` flag is
  set, which only `old_bash_script_offers_nothing_on_a_path_slot` sets.
- [ ] **Step 1:** Rework the driver as above. Keep `@QDRY` and `TW_TAIL` (tail words go into
      `COMP_LINE` after the cursor, and `COMP_POINT` stays at the end of the cursor word).
      Existing tests use `Split::Whole`, which matches what they test today.
- [ ] **Step 2:** Extend the fixture. Add `"a:b"` to `CHOICES`. Append `files/host:port.txt` to
      `PATH_FILES` (at the end, so the `quoted[0]` and `quoted[2]` indices keep their meaning).
      Update the `CHOICES.len()` and file-set assertions that count entries.
- [ ] **Step 3: Write the failing tests.** Each runs on every `bashes()` entry and both `Split`
      shapes:
    - `bash_colon_in_a_file_name_completes_after_the_colon`: `--cfg files/host:p` gives exactly
      `port.txt`. The sentinel stays absent.
    - `bash_colon_in_a_choice_completes_after_the_colon`: `--mode a:` gives `b`.
    - `bash_earlier_word_with_a_colon_stays_one_word`: typed
      `--cfg files/host:port.txt --mode fa` gives `fast`.
    - `bash_completion_ignores_a_user_ifs_without_a_space`: script prefixed with `IFS=$'\n'`.
      `--mode fa` gives `fast` and `--cfg files/host:p` gives `port.txt`.
    - Extend `bash_completion_survives_errexit` with the two colon cases under `-e` and `-eu`.
    - `bash_tilde_is_escaped`: the `x=~` and `x=\~` inputs now expect `\~root`, and the `""`
      input keeps `x=\~root`.
- [ ] **Step 4:** Run `cargo test -p toolr --test complete_shells`. Expected: every bash test
      except the `OLD_BASH_SCRIPT` one FAILS on every bash, with `_init_completion: command not
      found` on stderr, because the script still calls it until Task 2. That is the driver
      working, not a broken driver. Only the assertions listed in Step 3 change.
- [ ] **Step 5:** Do not commit yet; Task 2 makes them pass.

### Task 2: Script

**Files:**

- Modify: `crates/toolr-core/src/complete/scripts/bash.sh`

**Interfaces:**

- Consumes: the driver from Task 1.
- Produces: `_toolr_complete_paths MODE RAW_CUR [RAW_PREFIX]`.
- [ ] **Step 1:** At the top of `_toolr_complete`: `local IFS=$' \t\n'`, with a one-line comment
      on bash 3.2 joining a quoted slice with `IFS`'s first character.
- [ ] **Step 2:** Replace `_init_completion` and the `prev` lines with the rebuild loop from the
      spec. For `i` from 0 to `COMP_CWORD`: strip the leading whitespace of the remaining line
      into `ws`. At `COMP_CWORD` the word is the rest of the line. Otherwise it is
      `COMP_WORDS[i]`, consumed with `${line#"$w"}`. Append the word to `words[n - 1]` when
      `i > 0` and `ws` is empty, else start a new word. `cur` is the last rebuilt word.
- [ ] **Step 3:** In the unescape loop, set `raw_prefix=$raw_cur` after an unescaped character
      found in `breaks`, where `breaks` is `${COMP_WORDBREAKS-}` without whitespace, `'` and `"`.
      Build `args` with a loop over the rebuilt words 1 to `n - 2`, then `raw_cur`.
- [ ] **Step 4:** In the value loop, after the existing filter, re-quote the remainder when
      `raw_prefix` is set: `[[ -n $raw_prefix ]] && _toolr_quote "${value#"$raw_prefix"}"`.
      Pass `"$raw_prefix"` to both `_toolr_complete_paths` calls. There, take
      `raw_prefix=${3-}`, and when it's set use `rest=${name#"$raw_prefix"}` and skip the
      bare-`~` branch. Leave `is_dir`, `tilde_list` and `compopt` on the full `name`.
- [ ] **Step 5:** Run `cargo test -p toolr --test complete_shells`. Expected: PASS. Run
      `shellcheck crates/toolr-core/src/complete/scripts/bash.sh` (or check that prek runs it).
- [ ] **Step 6: Commit** the script and tests together as
      `fix(complete): bash completes words with colons and ignores the user's IFS` with a
      `Release-Notes: skip` trailer. The note lands in Task 4.

### Task 3: End-to-end pty test

**Files:**

- Modify: `crates/toolr/tests/complete_shells.rs`

**Interfaces:**

- Consumes: the script from Task 2 and `common::run_on_pty`.
- [ ] **Step 1: Write the test** `bash_tab_completes_a_colon_file_name_end_to_end`. For each
      `bashes()` entry, write an rcfile into the fixture that sources the printed script and
      defines
      `toolr() { if [[ $1 == __complete ]]; then command toolr "$@"; else printf 'LINE:%s\n' "$*"; fi; }`.
      Spawn `bash --noprofile --rcfile <file> -i` in the fixture root, with the toolr `PATH`, the
      fixture `HOME` and the test cache. Use `common::run_on_pty` with the preloaded input
      `toolr ci lint --cfg files/host:p\t\nexit\n`. Assert that stdout has the line
      `LINE:ci lint --cfg files/host:port.txt`. Repeat with `--mode a:b\t`, expecting
      `LINE:ci lint --mode a:b`.
- [ ] **Step 2:** Run it against `main`'s script first (temporarily paste it in, as
      `OLD_BASH_SCRIPT` does): the line stays `files/host:p`. Then against the new script:
      PASS. Remove the temporary paste.
- [ ] **Step 3: Commit** `test(complete): drive bash Tab completion through a pty` with a
      `Release-Notes: skip` trailer.

### Task 4: Release note

- [ ] **Step 1:** `toolr release-notes add --text "…"` with the spec's release-note text.
- [ ] **Step 2:** `toolr release-notes check --base main`. Expected: clean.
- [ ] **Step 3: Commit** `docs(release-notes): bash completes words with colons`.

### Task 5: Verify, then archive the specs

- [ ] **Step 1:** Run `mise run test`, polling the output every 30–60 seconds.
- [ ] **Step 2:** Run `prek run --all-files`. If the doc-snippets hook fails for a missing tools
      venv, run `toolr project venv sync -q` and retry.
- [ ] **Step 3:** Patch coverage: only `scripts/` and `crates/*/tests` change, and Codecov ignores
      both. If anything under `src/` changed, run the CI coverage command from CLAUDE.md and
      `cargo xtask patch-coverage --base main`.
- [ ] **Step 4:** `git mv specs/2026-11-04-bash-completion-words-design.md specs/archive/2026/`
      and the same for the plan. Commit `docs(specs): archive bash completion words spec and plan`
      with a `Release-Notes: skip` trailer. This is the final commit before the PR.
