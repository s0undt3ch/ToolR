# Plan: let the shell match completion candidates (FU-34)

Design: `specs/2026-11-06-zsh-matcher-list-design.md`. Build on `fix/complete-double-dash` (#691): rebase onto
`main` once it merges, or stack with git-spice. Work in a worktree under `.worktrees/`, at most three
sub-agents. Run `cargo` for the touched crates only, then `mise run test` at the end.

## Task 1: engine `Matching` (toolr-core)

Files: `crates/toolr-core/src/complete/engine.rs`, tests in `crates/toolr-core/src/complete/tests.rs`.

1. Add `Matching { Prefix (default), Shell }` with `fn keeps(self, candidate: &str, prefix: &str) -> bool`.
2. Add `pub fn complete_matching`; make `complete` and `serve_completions` delegate with `Prefix`.
3. Thread `matching` through `candidates`, `groups`, `commands`, `flags`, `group_node_flags`. Leave `classify`
   and `values`' `dash_ok` guard alone.
4. Re-export `Matching` and `complete_matching` from `crates/toolr-core/src/complete/mod.rs`.

Tests (fixture: the existing manifest helpers in `tests.rs`):

- `shell_matching_returns_every_group_for_any_prefix`: `complete_matching(&m, &["ZZZ"], Shell)` equals the
  `Prefix` result for `""`.
- `shell_matching_returns_every_command_in_a_group`: tokens `["example", "HE"]` returns all commands of
  `example`, sorted, deduplicated.
- `shell_matching_flag_slot_returns_every_flag_and_help`: tokens `[..cmd, "--zz"]` returns each flag plus
  `--help`.
- `shell_matching_non_flag_word_never_offers_flags`: tokens `[..cmd, "AL"]` on a command with a positional
  and flags returns the positional's values only.
- `shell_matching_flag_value_returns_every_choice`: `--mode` with `["fast","dry run"]` and word `ZZ`
  returns both.
- `shell_matching_attached_value_returns_every_choice`: word `--mode=ZZ` returns both bare values.
- `shell_matching_keeps_the_dash_guard`: choices `["-x","y"]`, word `ZZ` returns `["y"]`; word after a
  committed `--` returns both.
- `shell_matching_still_drops_unsafe_names`: a value with a control char is absent in both modes.
- `prefix_matching_default_is_unchanged`: `complete` equals `complete_matching(.., Prefix)` for the
  existing prefix cases (`ex` returns `example`, `EX` returns nothing).
- `shell_matching_paths_still_signal_the_shell`: a `Path` argument returns `Completion::Paths` in both modes.

## Task 2: binary reads the variable (toolr)

Files: `crates/toolr/src/dispatch.rs`, test in `crates/toolr/tests/complete_smoke.rs`, docs `docs/cli.md`.

1. `matching_from_env()`; pass to `complete_matching` and to `merge_root_flags_at_top_level`
   (new third parameter; skip its `starts_with` in `Shell`).
2. `docs/cli.md`, next to the exit-code table: document `TOOLR_COMPLETE_MATCH` and that unknown values mean
   prefix.

Tests in `complete_smoke.rs` (assert_cmd, real binary, `docs/.fixtures/sample-repo`-style fixture):

- `complete_defaults_to_prefix_filtering`: `__complete <cwd> EX` stdout is empty, exit 0.
- `complete_match_shell_returns_unfiltered_candidates`: same call with `TOOLR_COMPLETE_MATCH=shell` stdout
  contains `example\t`.
- `complete_match_unknown_value_means_prefix`: `TOOLR_COMPLETE_MATCH=banana` gives the empty stdout.
- `complete_match_shell_root_flags_unfiltered`: `__complete <cwd> --zz` with `shell` contains `--debug`
  and `--help`; without it, nothing.
- `complete_match_shell_keeps_path_exit_codes`: a path-typed slot exits 10 with and without the variable.

## Task 3: zsh script

Files: `crates/toolr-core/src/complete/scripts/zsh.zsh`, `crates/toolr/tests/complete_shells.rs`.

1. Prefix the `toolr __complete` call with `TOOLR_COMPLETE_MATCH=shell`.
2. Build `descriptions` only when `${#values} <= 20`. Keep `compadd -a values`, no `-Q`.
3. Measure 10 000 values (fixture `Literal` of that size) with the old loop and the expansion split via
   `zsh -f` and `EPOCHREALTIME`; keep the loop if within ~50 ms, else switch. Record both numbers in the PR.

Tests (stub driver `zsh_complete`; extend its stub `compadd` to also print the `values` array, one
`VALUE:<v>` line each, so a test sees the candidates):

- `zsh_script_asks_the_engine_for_shell_matching`: `print_script("zsh")` contains
  `TOOLR_COMPLETE_MATCH=shell`.
- `zsh_uppercase_partial_reaches_compadd`: words `toolr ci lint --mode FA` yields `VALUE:fast`, `VALUE:dry run`,
  `VALUE:dry walk` (all, unfiltered) and a `COMPADD:` line; on `main` the call list is empty.
- `zsh_attached_uppercase_partial_reaches_compadd`: `--mode=FA` gives a `COMPSET:-P 1 *=` line then the three
  values.
- `zsh_command_slot_offers_all_commands_for_any_partial`: `toolr ZZ` offers every top-level group.
- `zsh_path_slots_still_use_native_file_completion`: the existing test, unchanged, still passes.
- `zsh_completion_never_executes_values`: unchanged, still forbids `-Q` and `eval`.
- `zsh_large_choice_set_skips_descriptions`: 25 values give `COMPADD:-a values`.

Manual, in the PR description: in a real zsh with `zstyle ':completion:*' matcher-list 'm:{a-z}={A-Z}'`,
`toolr example EX<Tab>` completes `example`.

## Task 4: fish script (separate commit, gated)

Files: `crates/toolr-core/src/complete/scripts/fish.fish`, `crates/toolr/tests/complete_shells.rs`.

1. Re-run the fish repro first (`complete -C "toolr EX"` empty on `main`). If fish already matches, drop
   this task.
2. `set -l out (env TOOLR_COMPLETE_MATCH=shell toolr __complete "$PWD" $args 2>/dev/null)`.

Tests (existing `fish_complete` helper):

- `fish_script_asks_the_engine_for_shell_matching`: script contains `TOOLR_COMPLETE_MATCH=shell`.
- `fish_uppercase_partial_completes_case_insensitively`: line `toolr EX` prints a line starting `example`.
- `fish_substring_partial_completes`: `toolr xam` prints a line starting `example`.
- `fish_attached_value_keeps_opt_prefix`: `toolr ci lint --mode=FA` prints `--mode=fast`.
- `fish_path_flag_completes_files_inert`: unchanged.

## Task 5: compatibility tests

File: `crates/toolr/tests/complete_shells.rs`.

- `old_zsh_script_gets_prefix_filtered_output_from_new_binary`: run an inlined copy of the pre-change call (no
  variable) and assert the output equals today's (`FA` gives nothing, `fa` gives `fast`).
- `new_zsh_script_degrades_on_a_binary_that_ignores_the_variable`: stub `toolr` on `PATH` that ignores the
  environment and prints only `fast`; the script must still call `compadd` with `fast` and exit 0.

## Task 6: release note, skill refs, verify, archive

1. Queue a note with `toolr release-notes add --text "…"` saying: zsh and fish now let the shell match
   completion candidates, so zsh's `matcher-list` (case-insensitive, partial-word) and fish's fuzzy matching
   work on toolr commands, flags and choices; rerun `toolr self completion install` to pick up the new
   script; bash is unchanged.
2. `cargo xtask build-skill-refs --check`: no public `__all__` change, expect clean.
3. No employer names and no absolute home paths in the diff.
4. `rustfmt --edition 2024` on touched files only; `prek run --all-files`.
5. `mise run test` (Rust and Python touched). Monitor the cargo run for stalls.
6. Patch coverage: `cargo xtask patch-coverage --base <base>`; every new engine branch is hit by the Task 1
   tests. Do not run `cargo llvm-cov` on a low-disk machine; let CI do it.
7. Archive both spec files with `git mv` to `specs/archive/2026/` as the last commit before opening the PR.

## Commit order

1. `feat(complete): let the shell match candidates when asked` (Tasks 1 and 2)
2. `feat(complete): zsh matcher-list applies to toolr candidates` (Task 3, 5)
3. `feat(complete): fish fuzzy matching applies to toolr candidates` (Task 4)
4. `docs(release-notes): shell-side completion matching`
5. `docs(specs): archive the shell-matching design`
