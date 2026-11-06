# Bash path completion: quoted and escaped words: implementation plan

> **For agentic workers:** use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bash path and value completion works for words holding a backslash escape or an open quote, on every
supported bash (audit FU-24 remainder).

**Architecture:** `_toolr_complete_paths` lists paths with a literal glob in a subshell instead of `compgen`, and the
cursor-word parser in `_toolr_complete` understands quotes and quotes each reply for the open quote readline is in.

**Spec:** `specs/2026-11-06-bash53-compgen-dequote-design.md`. Its "Decisions" section records the user's calls.

## Global constraints

- Branch `fix/bash53-compgen-dequote`, worktree `.worktrees/bash53-compgen-dequote`.
- Stage by path, never `git add -A`, never stage `audit/`. Conventional Commits, no Co-Authored-By trailer. British
  English. Prefix `git commit` with `TOOLR_TRUST_ALL=1`.
- Format only touched files (`rustfmt --edition 2024 crates/toolr/tests/complete_shells.rs`).
- Never background test runs with `&`; run them in the foreground.
- The `$`/backtick guard in `_toolr_complete_paths` stays as it is.
- Run every bash test against bash 5.1.16, 5.2.37 and 5.3 from mise, plus whatever `bashes()` finds:

```sh
for v in 5.1.16 5.2.37 5.3; do
    PATH="$HOME/.local/share/mise/installs/conda-bash/$v/.mise-bins:$PATH" \
        cargo test -p toolr --test complete_shells bash_ || break
done
```

  Install them first with `mise install conda:bash@5.1.16 conda:bash@5.2.37 conda:bash@5.3`. Use the full path, not
  `mise x`, from inside the repo: `mise x` there rewrites `mise.lock` targets.

## Task 1: failing tests

**Files:** `crates/toolr/tests/complete_shells.rs`

- [ ] Append `"files/it's.txt"` and `"files/q\"d.txt"` to `PATH_FILES` (now 8 entries). Existing `quoted[0]` and
      `quoted[2]` uses keep their meaning.
- [ ] Extract `fn bash_tab_rc(fx: &Fixture) -> String` (the rcfile body) and
      `fn assert_bash_tab_line(bash, fx, rcfile, typed, want)` (the `StdCommand` setup, `run_on_pty` and the `LINE:`
      assertion) from `bash_tab_completes_words_with_a_colon_end_to_end`. The colon test must pass unchanged.
- [ ] Add `bash_tab_completes_quoted_and_escaped_paths_end_to_end`. For each `bashes()` entry, type
      `toolr ci lint <word>`, then Tab and Enter, and assert a `LINE:` line equal to the expected one (`H` is
      `fx.tmp.path()`):

| Typed word             | Expected `LINE:`                          |
| ---------------------- | ----------------------------------------- |
| `--cfg files/back\\s`  | `ci lint --cfg files/back\slash`          |
| `--cfg files/it\'`     | `ci lint --cfg files/it's.txt`            |
| `--cfg "files/wi`      | `ci lint --cfg files/with space.txt`      |
| `--cfg 'files/wi`      | `ci lint --cfg files/with space.txt`      |
| `--cfg files/"wi`      | `ci lint --cfg files/with space.txt`      |
| `--cfg "files/it`      | `ci lint --cfg files/it's.txt`            |
| `--cfg 'files/it`      | `ci lint --cfg files/it's.txt`            |
| `--cfg 'files/q`       | `ci lint --cfg files/q"d.txt`             |
| `--cfg "$HOME/files/wi` | `ci lint --cfg H/files/with space.txt`   |
| `--cfg=files/back\\s`  | `ci lint --cfg=files/back\slash`          |
| `--mode "dry r`        | `ci lint --mode dry run`                  |
| `--cfg ~/files/wi`     | `ci lint --cfg H/files/with space.txt`    |

  Leave out words that should offer nothing: with an open quote, Enter would wait for a continuation line.

- [ ] Add `Drive` tests (they also run on Linux CI bash 5.2, where the pty rows above are the real oracle):
    - `bash_path_reply_is_quoted_for_an_open_double_quote`: cur `"files/wi` gives reply `["files/with space.txt"]`;
      cur `"files/it` gives `["files/it's.txt"]`; cur `"files/q` gives the reply text `files/q\"d.txt`.
    - `bash_path_reply_is_quoted_for_an_open_single_quote`: cur `'files/it` gives the reply text `files/it'\''s.txt`;
      cur `'files/wi` gives `["files/with space.txt"]`.
    - `bash_path_open_quote_mid_word_replaces_after_the_quote`: cur `files/"wi` gives `["with space.txt"]`.
    - `bash_path_quoted_tilde_is_literal`: with a literal directory `~/files` made under the fixture (`literal_dir`),
      cur `"~/fi` gives `["~/files/"]` from the fixture directory, not the home one; with no such directory, nothing.
    - `bash_path_dollar_in_single_quotes_offers_nothing`: cur `'$HOME/fi` gives an empty reply, rc `0`.
    - `bash_path_variable_after_a_double_quote_stays_bare`: cur `"$HOME/files/s` gives `["$HOME/files/sub/"]` and
      `compopt` `["-o nospace"]`.
    - `bash_value_reply_is_quoted_for_an_open_quote`: `run_bash` with cur `"dry r` or `'dry r` gives `["dry run"]`.
    - `bash_path_name_with_a_newline_is_one_candidate`: create `files/nl\nx`; cur `files/nl` gives exactly one reply,
      `$'files/nl\nx'` (`printf %q` form).
- [ ] Run the loop from "Global constraints". Expect the new pty test to fail on every version and most new `Drive`
      tests to fail. Note which, and keep that list for the PR body.
- [ ] Commit: `test(complete): reproduce bash completion of quoted and escaped paths`.

## Task 2: glob listing in `_toolr_complete_paths`

**Files:** `crates/toolr-core/src/complete/scripts/bash.sh`

- [ ] Replace the `compgen "$_toolr_mode" -- "$_toolr_raw_cur"` feed with the subshell glob from the spec
      ("1. List paths with a glob"). Read with `while IFS= read -r -d '' _toolr_name`.
- [ ] Split `_toolr_raw_cur` into `_toolr_dir` (through the last `/`, empty when there is none) and `_toolr_leaf`.
- [ ] Drop `.` and `..` from the glob, and add `$_toolr_dir.` and `$_toolr_dir..` back when `_toolr_leaf` starts with
      `.` and is a prefix of them.
- [ ] `-d` mode: skip names failing `[[ -d $_toolr_name ]]`.
- [ ] Tilde: a bare `~` with `HOME` set globs under `$HOME` and maps each name back to the typed `~` before the
      `_toolr_raw_prefix` strip. `~user` (and `~` with `HOME` unset) keeps the `compgen` listing and its `compgen -d`
      probe, only when the name matches `^[A-Za-z0-9._][A-Za-z0-9._-]*$` and the rest holds no `\`, `'` or `"`;
      otherwise it offers nothing. No `eval`.
- [ ] `$NAME`: drop the `./` prefix for a value starting with `~`; keep the rest of the `$NAME` logic.
- [ ] Keep the `$`/backtick guard line and its comment's first sentence; reword its second sentence to say the guard
      predates the glob and is kept on purpose.
- [ ] `compgen` appears only on the `~user` path.
- [ ] Run the loop. The backslash rows (`files/back\\s`, `--cfg=files/back\\s`) and the newline `Drive` test pass. All
      existing `bash_` tests pass.
- [ ] Commit: `fix(complete): list bash path candidates with a literal glob, not compgen`.

## Task 3: quote-aware word parsing and replies

**Files:** `crates/toolr-core/src/complete/scripts/bash.sh`

- [ ] Rewrite the unescape loop in `_toolr_complete` as the parser in the spec ("2. Parse quotes"). It sets
      `_toolr_raw_cur`, `_toolr_raw_prefix`, `_toolr_qchar`, `_toolr_tilde_ok` and `_toolr_var_ok`. Word breaks apply
      only outside quotes. An open quote sets `_toolr_raw_prefix` to the raw text before it.
- [ ] `_toolr_quote` and `_toolr_quote_after` read `_toolr_qchar` from the caller's scope (dynamic scoping, as
      `_toolr_q` is already written) and quote for none, `"` or `'`. Keep the bash 4 `~` fix for the none case only.
- [ ] Pass `_toolr_qchar`, `_toolr_tilde_ok` and `_toolr_var_ok` to `_toolr_complete_paths` as extra arguments.
      Update its usage comment. `_toolr_var_re` is applied only when `_toolr_var_ok`, and the tilde branch only when
      `_toolr_tilde_ok`.
- [ ] Fix the stale comment "Readline doesn't break inside an open quote, so such a word is left untrimmed": say
      instead that readline replaces from just after an open quote.
- [ ] Run `shellcheck crates/toolr-core/src/complete/scripts/bash.sh` (via `prek run shellcheck --files …` if the
      hook exists).
- [ ] `bash_script_names_its_variables_with_the_toolr_prefix` reads the word after `read`; teach it to skip the
      argument of `-d`, so `read -r -d ''` isn't taken for a name.
- [ ] Run the loop. Every test from Task 1 passes on 5.1.16, 5.2.37 and 5.3, and every existing `bash_` test,
      including `bash_script_names_its_variables_with_the_toolr_prefix` (new locals need the `_toolr_` prefix).
- [ ] Commit: `fix(complete): quote bash completion replies for an open quote`.

## Task 4: release note, full verification, archive

- [ ] `toolr release-notes add --text "Bash tab completion now completes paths typed with a backslash escape or inside
      quotes (\"files/my f<TAB>\", files/it\\'s<TAB>), and no longer inserts backslashes inside an open quote."`
      Commit: `docs(release-notes): bash completion of quoted and escaped paths`.
- [ ] `toolr release-notes check --base main`.
- [ ] `mise run test` (foreground; poll the output every 30–60s).
- [ ] `prek run --all-files`.
- [ ] Final commit: `git mv` both spec files to `specs/archive/2026/`, commit
      `docs(specs): archive the bash compgen dequoting spec` with a `Release-Notes: skip` trailer.

Patch coverage: `bash.sh` sits under `crates/toolr-core/src/complete/scripts`, which Codecov ignores, and the tests
are under `crates/*/tests`, so this PR adds no covered lines.
