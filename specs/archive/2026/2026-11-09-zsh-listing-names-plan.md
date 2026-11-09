# Plan: show names next to descriptions in zsh listings (FU-46)

Design: `specs/2026-11-09-zsh-listing-names-design.md`.
Work in the worktree `.worktrees/zsh-listing-names` on branch `fix/zsh-listing-names` (git-spice, base `main`).
Sub-agents: none needed; the change is one script branch and its tests.

## Global Constraints

- Only the 20-or-fewer branch of `crates/toolr-core/src/complete/scripts/zsh.zsh` changes.
- Keep `_description toolr expl toolr`, `"${expl[@]}"` on every `compadd`, `-O matched`, no `-Q`,
  `compset -P 1 '*='` before `compadd`, and the `loadautofunc` guard.
- No `eval`, no expansion of candidate text. `zsh_completion_never_executes_values` must stay green.
- Run `prek` for lint and format, never `ruff` or `cargo fmt` directly. Tests run directly.
- Python tests on 3.11 and the newest only if Python is touched; it isn't.
- British English, no employer name, no absolute home paths in files.
- Stage files by path; no `git add -A`; no Co-Authored-By; no `--no-verify`.

## Review Focus

- Does `-l` still honour `matcher-list` and the neutral `toolr` header, in a live zsh?
- Alignment with wide characters and with an empty description.
- Any remaining test that expects `-d descriptions`.
- The 20-match boundary: exactly 20 gets names and descriptions, 21 gets the grid.

## Task 1: failing live test (pins the bug)

File: `crates/toolr/tests/complete_shells.rs`, next to `zsh_tab_lists_descriptions_when_the_matches_are_few`.

1. Add `zsh_tab_lists_each_name_with_its_description`. Use `big_described_group` and `zsh_session` with
   `ZshLoad::Sourced`, type `toolr big cmd0\t\x18dexit\n`, read the `tty` half.
2. Assert some line of `tty` contains both `cmd03` and `Summary number 03.`, in that order, and that the
   line carries no other `cmd` name. Assert the same for the `matcher-list` rc with `CMD0`.
3. Add `zsh_tab_one_row_per_match`: with `cmd0` there are 10 matches, so the listing has 10 rows that each
   hold a `cmdNN` name and its own `Summary number NN.`.
4. Run `cargo test -p toolr --test complete_shells zsh_tab_lists_each_name -- --test-threads=1`. It must fail
   on `main`: `tty` holds `Summary number 03.` and no `cmd03` on that line.

## Task 2: stub-driver expectations

File: `crates/toolr/tests/complete_shells.rs`, `zsh_run_script` and the tests below.

1. In the `compadd` stub, also print each element of `display` as a `DISP:` line, and keep the `DESC:` lines
   from `descriptions`.
2. Replace every `COMPADD:-d descriptions -a values` expectation with `COMPADD:-l -d display -a values`
   (`zsh_attached_option_with_escaped_value_matches`, `zsh_path_slots_use_native_file_completion`,
   `zsh_attached_value_moves_the_option_into_the_prefix`, `zsh_descriptions_follow_their_values`,
   `zsh_uppercase_partial_reaches_compadd` and the matcher-list stub test). Grep for `-d descriptions` to be
   sure none is missed.
3. In `zsh_descriptions_follow_their_values` assert `DISP:lint  -- Lint things.` style lines, padded to the
   widest value, and `DISP:<value>` for a value with no description.
4. Add `zsh_value_with_a_tab_never_reaches_compadd`: a fixture choice containing `\t` is absent from `VALUE:`
   lines and its neighbours keep their own descriptions. This pins the tab decision.
5. Run the zsh stub tests. The `COMPADD` expectations fail until Task 3.

## Task 3: the fix

File: `crates/toolr-core/src/complete/scripts/zsh.zsh`, the `<= 20` branch.

1. Keep `descriptions=("${(@)${(@)lines%$'\t'}#*$'\t'}")`.
2. Add `local -a display; local -i width=0 i` and a loop over `values` that sets `width` to the widest
   `${(m)#value}`.
3. Fill `display[i]` with `${(mr:width:)value}  -- ${descriptions[i]}`, or `${values[i]}` when
   `descriptions[i]` is empty.
4. Change the call to `compadd "${expl[@]}" -l -d display -a values`.
5. Update the comment above the branch: it says `-d` displays the array, so the array carries the name.
6. Run `cargo test -p toolr --test complete_shells zsh -- --test-threads=1`. Tasks 1 and 2 now pass.
7. Probe by hand in `zsh -f -i`: `%` in a description, a value with a colon, a wide-character name. Record
   the result in the PR; if `%` expands, double it in `display`.

## Task 4: boundary and regression checks

1. Add `zsh_tab_names_appear_at_exactly_twenty_matches`: 20 commands show `cmd00 ... Summary` rows, and 21
   fall back to the grid (`cmd20` shown, no `Summary number`). Extend `big_described_group` with a count
   argument if needed, keeping the existing 25 as the default.
2. Confirm `zsh_tab_header_is_neutral_for_every_slot_kind`, `zsh_tab_applies_the_matcher_list`,
   `zsh_tab_completes_quoted_and_escaped_partials_end_to_end` and `zsh_first_tab_completes_when_autoloaded_or_sourced`
   pass unchanged.

## Task 5: release note

Run:

```sh
toolr release-notes add --text "zsh completion lists each command, flag and value next to its description again; a short list showed only the descriptions."
```

Then `toolr release-notes check --base main`. Commit as `docs(release-notes): zsh listings show names`.

## Task 6: verification

1. `mise run test` (Rust changed). Poll output every 30 to 60 seconds; do not background with `&`.
2. `prek run --all-files`.
3. `cargo xtask build-skill-refs --check` (runs inside `mise run test`; no public surface changes).
4. Patch coverage: the script directory is ignored by Codecov, the test changes count. Run
   `cargo xtask patch-coverage --base main` against the lcov file if Rust sources outside tests changed;
   none are expected.
5. Grep the diff for the employer name and for absolute home paths; both must be absent.

## Task 7: archive specs

Last commit before opening the PR:

```sh
git mv specs/2026-11-09-zsh-listing-names-design.md specs/archive/2026/
git mv specs/2026-11-09-zsh-listing-names-plan.md specs/archive/2026/
```

Commit as `docs(specs): archive zsh listing names spec and plan`. Mark the design `Status: implemented (PR #NNN)`
before the move. Open the PR with `git-spice branch submit --draft`, three review rounds, then
`gh pr ready`; never back to draft.
