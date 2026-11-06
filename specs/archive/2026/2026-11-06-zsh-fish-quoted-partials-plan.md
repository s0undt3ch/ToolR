# Plan: zsh and fish quoted partials (FU-33)

Design: `specs/2026-11-06-zsh-fish-quoted-partials-design.md`. Branch `fix/zsh-fish-quoted-partials`, one PR,
based on `main`, which now includes #690.

## Files

- `crates/toolr-core/src/complete/scripts/zsh.zsh`
- `crates/toolr-core/src/complete/scripts/fish.fish`
- `crates/toolr/tests/complete_shells.rs`
- `release-notes/<generated>.md` via `toolr release-notes add --text "…"`
- No engine, Rust source or `RUNNER_SCHEMA_VERSION` change. The scripts are `include_str!`'d, so `cargo test -p toolr-core`
  recompiles them; the skill-refs drift gate is unaffected.

## Tests

CI provides the shells: `.github/workflows/_test.yml` installs `fish zsh` with apt on Linux and `brew install bash fish`
on macOS (zsh ships with macOS). The `require()` helper in `complete_shells.rs` turns a missing shell into a failure
when `CI` is set. All tests below use `require("zsh")` or `require("fish")`.

Extend the zsh driver `zsh_complete` so a test can set `compstate[quote]` (declare `typeset -A compstate` in the
`DRIVER` string and take the value from an env var). Add a second recorder that echoes the `values` passed to
`compadd -a`, so assertions see the candidates, not only the flags.

Write the failing tests first.

### zsh (real zsh, stubbed `compadd`/`_files`)

- `zsh_backslash_escaped_partial_matches_a_choice`: words `toolr ci lint --mode 'dry\ r'`; assert candidates are exactly
  `["dry run"]`.
- `zsh_open_single_quote_partial_matches_a_choice`: `compstate[quote]="'"`, word `'dry r`; assert `["dry run"]`.
- `zsh_open_double_quote_partial_matches_a_choice`: `compstate[quote]='"'`, word `"dry r`; assert `["dry run"]`.
- `zsh_closed_quote_partial_matches_a_choice`: word `'dry r'`; assert `["dry run"]`.
- `zsh_attached_option_with_escaped_value_matches`: word `--mode=dry\ r`; assert `["dry run"]` and a `COMPSET:-P 1 *=`
  line (the `attached` branch still fires on the dequoted word).
- `zsh_earlier_escaped_words_reach_the_engine_unescaped`: words `toolr ci lint --cfg a\ b --mode 'dry\ r'`; assert
  `["dry run"]` (an escaped earlier word must not shift slot detection).
- `zsh_backtick_quote_offers_nothing`: `compstate[quote]` a backtick; assert no `COMPADD` line.
- `zsh_dollars_quote_partial_matches_a_choice` (only if open question 2 resolves to support): `$'dry r`.
- Keep `zsh_completion_never_executes_values` unchanged (still asserts no `-Q`, no `eval `).
- Keep `zsh_path_slots_use_native_file_completion` unchanged (paths still go to `_files` with `-`/`-/`).

### fish (real fish, `complete -C`)

- `fish_backslash_escaped_partial_matches_a_choice`: line `toolr ci lint --mode dry\ r`; stdout names are `["dry run"]`.
- `fish_single_quoted_partial_matches_a_choice`: `toolr ci lint --mode 'dry r`; `["dry run"]`.
- `fish_double_quoted_partial_matches_a_choice`: `toolr ci lint --mode "dry r`; `["dry run"]`.
- `fish_attached_option_with_escaped_value_keeps_its_prefix`: `toolr ci lint --mode=dry\ r`; names are `["--mode=dry run"]`.
- `fish_escaped_path_partial_still_completes`: `toolr ci lint --cfg files/with\ s`; names contain
  `"files/with space.txt"` (guards the raw-word-for-paths rule).
- `fish_quoted_path_partial_still_completes`: `toolr ci lint --cfg 'files/with s`; same assertion.
- Existing `fish_completion_never_executes_values` and `fish_path_flag_completes_files_inert` stay as they are;
  `$(touch …)` values must still not run when the partial is quoted. Add one more case there: partial `'$(touch`
  with the sentinel asserted absent.

### Live Tab (in the suite)

An interactive zsh on a pty, as the bash Tab tests use, confirms that Tab on `toolr ci lint --mode dry\ w`, `'dry w`,
`"dry w` and `$'dry w` inserts a correctly quoted `dry walk`, per open question 1. It proved reliable, so it runs in
the default suite as `zsh_tab_completes_quoted_and_escaped_partials_end_to_end` rather than behind an opt-in.

## Tasks

1. Try to get a `zpty` probe working to settle the unverified premise (PREFIX dequoted, `compadd` re-quotes) and open
   question 2. Time-box it; record the outcome in the PR.
2. Extend the zsh driver, write the zsh tests, watch them fail on `main`.
3. Edit `zsh.zsh`: add `_toolr_unquote` (or inline) that appends the closing quote per `compstate[quote]` then applies
   `(Q)`; use it for `cur` and `passthrough`; return early on `backtick`. Keep `compadd` without `-Q`.
4. Write the fish tests, watch them fail. Edit `fish.fish`: `raw`/`current` split, engine and reply branch on `current`,
   path branches on `raw`. Run on fish 3.x if available (the file already cares about < 3.7); `string unescape` exists
   since 2.x.
5. Verify: `cargo test -p toolr --test complete_shells -- --test-threads=1`, then `mise run test` (touches Rust test code).
   `rustfmt --edition 2024 crates/toolr/tests/complete_shells.rs` only; no whole-crate fmt.
6. Coverage: scripts under `crates/toolr-core/src/complete/scripts` are ignored by Codecov, so the 99% patch gate sees
   only the test file. No uncovered Rust lines are added.
7. `toolr release-notes add --text "zsh and fish completion matches choices typed with a backslash or quote."`
   Run `toolr release-notes check --base main`.
8. Grep the diff for employer names, then archive both spec files with `git mv` into `specs/archive/2026/` as the last commit.
9. Self-review rounds, then open the draft PR (`git-spice branch submit --draft`).

## Done when

- Every row of the design's reproduction table that said "none" returns the expected candidate under the new tests.
- The existing never-executes and native-path tests pass unchanged.
- FU-34 is untouched and cross-referenced in the PR description.
