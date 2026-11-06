# Bash completion: ANSI-C quoted words: plan

Implements [`2026-11-06-bash-ansi-c-words-design.md`](2026-11-06-bash-ansi-c-words-design.md) (FU-40). Branch from
PR #690's tip (`fix/bash53-compgen-dequote`) or from `main` once it merges; every test below uses that script's
helpers. All tests are in `crates/toolr/tests/complete_shells.rs`, run on every bash `bashes()` finds, and create
their files with `std::fs::write` as the neighbouring newline tests do. File names: `files/an\nd`, `files/an\nx`,
`files/nl\nx`, `files/nl\tz`, `files/solo\nname`, `files/exact`, `files/exact2`, `files/tab\there`, and a directory
`files/nl\nd/` holding `in`. (`files/sub/` already exists in the fixture.)

## Step 0: confirm the baseline

- [ ] On `main` and on #690, run `_toolr_complete` for `~"root"/` and `~$'root'/` after `--cfg` with a `compgen`
      function that logs its arguments. If `compgen` is called, the tilde hardening in step 2 also fixes that
      case: add `bash_path_tilde_with_a_quoted_part_never_reaches_compgen` to step 1.

## Step 1: tests first (all fail on #690)

Direct-call tests use `assert_path_reply` (exactly one reply) or `run_bash_cfg_path` (a set).

- [ ] `bash_path_word_in_an_open_ansi_c_quote_is_listed`. `$'files/an` replies `files/an\nd` and `files/an\nx`
      (escaped for the inside of the run, nothing before it), and `$'files/solo\nna` replies `files/solo\nname`.
- [ ] `bash_path_open_ansi_c_quote_after_a_prefix_replies_only_the_rest`. `~/$'files/solo` and
      `$HOME/$'files/solo` reply `files/solo\nname`; `files/$'solo` replies `solo\nname`.
- [ ] `bash_path_closed_ansi_c_word_keeps_completing`. `$'files/nl\nd'/` replies `files/nl$'\n'd/in`, and the same
      with `~/` and `$HOME/` in front of the `$'…'` (replies keep `~/` / `$HOME/` bare). A user-named variable
      `$NAME/$'…'/` too.
- [ ] `bash_path_ansi_c_escapes_decode`. Table, each offering one file: `$'files/tab\x09h` and `$'files/tab\011h`
      and `$'files/tab\th` reply `files/tab\there`; `$'files/it\'s` replies the `it's` file with `\'`
      (superseded: a typed `\'` is declined and a reply writes `'` as `\047`; see the design's "Changes during
      implementation"); `$'files/an\\` style typed backslash matches a `back\slash` file; a dangling
      `$'files/solo\` replies `files/solo\nname`.
      `\u` with `LC_ALL=C.UTF-8` on a file `files/é.txt`: `$'files/é` replies it; the same under `LC_ALL=C`
      offers nothing.
- [ ] `bash_path_undecodable_ansi_c_escape_offers_nothing`. `$'files/\q`, `$'files/nl\x00`, `$'files/\0`, `$'files/\cA`
      give empty replies and rc 0.
- [ ] `bash_path_ansi_c_word_never_runs_a_substitution`. Extends the style of
      `bash_path_never_runs_a_typed_substitution`: `$'$(touch SENTINEL)`, `$'\x24(touch SENTINEL)'`,
      `$'\044(touch SENTINEL)'`, `$'$(touch SENTINEL)'`, ``$'\x60touch SENTINEL2\x60'`` and ``$'`touch SENTINEL2`'``
      all reply nothing and create neither sentinel; run under bash 5.3 too, where `compgen` expands.
- [ ] `bash_path_decoded_dollar_is_not_a_variable`. With `NAME` set to the fixture root, `$'\x24NAME/files/an`
      offers nothing (no listing under the variable); `$NAME/$'files/an` still lists, via the typed form.
- [ ] `bash_path_bare_reply_groups_each_control_character`. `files/an` replies `files/an$'\n'd` and
      `files/an$'\n'x`; `files/nl` replies the three `nl` names grouped (`files/nl$'\n'x`, …, `files/nl$'\t'z`).
- [ ] `bash_path_common_prefix_of_grouped_replies_is_closed`. Computes the longest common prefix of the replies for
      `files/an` in the test and asserts `files/an$'\n'` and that its count of unescaped `$'` openers equals its
      closers. A second case pins the documented residual: `a\nb`/`a\tb` replies are both offered and their
      common prefix is `a$'\`.
- [ ] `bash_path_controls_in_open_double_and_single_quotes_close_and_reopen`. `"files/an` replies
      `files/an"$'\n'"d`; `'files/an` replies `files/an'$'\n''d`.
- [ ] `bash_path_exact_match_under_an_open_quote_is_not_an_empty_reply`. `files/exact'` with only `files/exact`
      present: no replies. With `files/exact2` too: exactly one, `2`. `files/sub'` replies `/`. Same under `"`
      and `$'` (`files/exact$'`). No reply is ever an empty string.
- [ ] Superseded, never written: the engine drops control-character values, so
      `bash_value_completes_in_ansi_c_quotes` replaced it. `bash_value_with_a_newline_completes_in_ansi_c_quotes`.
      A `Literal` value holding a newline: `--mode $'a` and
      the bare form give the grouped or escaped reply as above.

End-to-end (pty), using `assert_bash_tab_line` with a new `bash_tab_rc_quoted` whose `toolr` shim prints
`LINE:` and then each argument through `%q`, so a newline in an argument stays on one line:

- [ ] `bash_tab_never_leaves_an_ansi_c_quote_open_end_to_end`. `--cfg files/an` + Tab + Enter runs the shim with
      `LINE: ci lint --cfg $'files/an\n'`: no `PS2>`, no hang.
- [ ] `bash_tab_completes_ansi_c_quoted_paths_end_to_end`. Each of `--cfg $'files/so`, `--cfg files/$'so`,
      `--cfg $'files/solo\`, `--cfg ~/$'files/so`, `--cfg $HOME/$'files/so` and `--cfg ~/$'files/nl\nd'/` completes
      to the right argument (`{home}` substituted).
- [ ] `bash_tab_keeps_a_bang_literal_in_ansi_c_quotes_end_to_end`. `set -H`, a name with `!`, typed inside and
      before `$'` (open question 2).

## Step 2: `_toolr_parse_word`

- [ ] Add the `$'` opener and decoder per the design table; set `_toolr_declined`; keep every `_toolr_`-prefixed
      local (the `bash_script_names_its_variables_with_the_toolr_prefix` test must stay green).
- [ ] `_toolr_complete` returns early on `_toolr_declined`, before `toolr __complete`.
- [ ] Compute `_toolr_var_ok` / `_toolr_tilde_ok` from `_toolr_cur` with the tightened patterns.

## Step 3: `_toolr_quote`

- [ ] Add the `$'` context and the per-character grouping for closed, `"` and `'` forms; the fast path stays the
      single `printf %q`. Re-run `bash_path_completion_scales_linearly_with_the_listing` and compare timing.

## Step 4: exact match

- [ ] Skip a candidate equal to `_toolr_raw_prefix` unless it is a directory; the directory reply is `/`.

## Step 5: verify

- [ ] Resolve open question 2 by running the bang test on 5.1.16, 5.2.37 and 5.3.
- [ ] Re-run round 2's pty harness (`h2.py`) over the repro table; each row now completes and none ends in `PS2>`.
- [ ] `mise run test` (umbrella). `prek run --all-files`. Patch coverage: the new branches are shell, so they are
      covered only by the tests above; no Rust lines change.
- [ ] `toolr release-notes add --text "Bash completion handles names with newlines or control characters: …"`.
- [ ] `git mv` both spec files to `specs/archive/2026/` as the last commit.
