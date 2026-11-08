# Root flags before the first group: plan

Implements [`2026-11-06-root-flags-after-root-flag-design.md`](2026-11-06-root-flags-after-root-flag-design.md)
(audit FU-39). Branches from `main`; #691 (`fix/complete-double-dash`) has merged.
Commit as `fix(complete): offer groups and root flags after a root flag`.

## Files

- `crates/toolr/src/builtin_completions.rs`: add `RootScan`, `scan_root_flags`, `RootScan::offerable_long_flags` (the merge's
  filter) and a private `root_flag_specs()`; unit tests.
- `crates/toolr/src/dispatch.rs`: `run_complete` scans and strips; `merge_root_flags_at_top_level` becomes
  `merge_root_flags(rest, &scan, candidates)`.
- `crates/toolr/tests/complete_smoke.rs`: smoke tests below.
- `release-notes/`: `toolr release-notes add --text "Tab completion now offers groups, commands and further root flags
  after a root flag (\`toolr --quiet <TAB>\`, \`toolr --quiet --<TAB>\`), and no longer offers a root flag that was
  already given or conflicts with one."`
- No engine, script or schema change; no skill-refs change (no `__all__` touched).

## Steps

1. **`root_flag_specs()`**: from `crate::cli::build_command(&empty_manifest())`, one spec per root arg: `id`, `long`,
   `short`, `aliases` (`get_all_aliases`), `takes_value` (`get_num_args` non-zero / `get_action().takes_values()`),
   `conflicts` (`cmd.get_arg_conflicts_with(arg)` ids, mirrored so the lists are symmetric: clap's own is not on an
   unbuilt command).
2. **`scan_root_flags(tokens)`** per the design's rules. Committed tokens are `tokens[..len-1]`; `rest` is the
   remaining committed tokens plus the last token.
3. **`run_complete`**: after `raw_complete_tokens`, `let scan = scan_root_flags(&tokens);`; if `scan.dead_end`, return
   `ExitCode::SUCCESS` with no output; call `complete(&manifest, &scan.rest)`; merge with `scan.rest`.
4. **`merge_root_flags`**: condition `rest.len() == 1` and flag prefix; filter by `seen` and conflicts; drop `--help`
   when `help` is seen; keep `sort_and_dedup_by_value`.
5. Release note, then `mise run test` (touches Rust), `prek run --all-files`.

## Unit tests (`builtin_completions.rs`, `#[cfg(test)]`)

Helper `scan(&["--quiet", "--"])` builds `Vec<String>`; assert on `rest`, `seen` (sorted) and `dead_end`.

| Test                                     | Input tokens                       | Assertions                                                  |
| ---------------------------------------- | ---------------------------------- | ----------------------------------------------------------- |
| `scan_strips_leading_root_flags`         | `--quiet --timestamps ci ""`       | `rest == ["ci", ""]`, `seen == ["quiet","timestamps"]`, not dead |
| `scan_in_progress_word_is_never_consumed`| `--quiet --timeo`                  | `rest == ["--timeo"]`, `seen == ["quiet"]`                  |
| `scan_resolves_aliases_and_shorts`       | `--ts -q --nots 5 ""`              | `seen == ["no-output-timeout-secs","quiet","timestamps"]`, `rest == [""]` |
| `scan_value_flag_takes_next_token`       | `--timeout-secs 5 ""`              | `rest == [""]`, `seen == ["timeout-secs"]`                  |
| `scan_attached_value`                    | `--timeout-secs=5 ""`              | `rest == [""]`                                              |
| `scan_value_owed_is_dead_end`            | `--timeout-secs ""`                | `dead_end`                                                  |
| `scan_bad_value_is_dead_end`             | `--timeout-secs abc ""`, `--timeout-secs= ""`, `--timeout-secs -5 ""`, `--timeout-secs --quiet ""` | each `dead_end` |
| `scan_duplicate_is_dead_end`             | `--quiet --quiet ""`, `--ts --timestamps ""`, `--help -h ""`, `--timeout-secs 1 --timeout 2 ""` | each `dead_end` |
| `scan_conflict_is_dead_end`              | `--quiet --debug ""`, `-qd ""`, `--ts --nts ""`, `-d --quiet ""` | each `dead_end` (both orders) |
| `scan_stops_at_terminator`               | `--quiet -- ""`                    | `rest == ["--", ""]`, `seen == ["quiet"]`                   |
| `scan_stops_at_first_word`               | `--quiet ci --debug ""`            | `rest == ["ci","--debug",""]`, `seen == ["quiet"]`          |
| `scan_stops_at_unknown_flag_or_short`    | `--bogus --quiet ""`, `-qx ""`, `- ""` | `rest` unchanged (starts at that token), not dead      |
| `root_flag_conflicts_are_symmetric`      | specs                              | for every spec pair, `a` conflicts `b` iff `b` conflicts `a` (guards the mirroring) |
| `scan_accepts_attached_negative_values`  | `--timeout-secs=-1 ""`, `--nots=-1 ""`, `--timeout-secs=-1e3 ""`; `--timeout-secs=-x ""` | first three not dead, `rest == [""]`; last dead |
| `scan_version_ends_the_line`             | `--version ""`, `-V ""`, `-qV ""`, `--quiet --version --` | each `dead_end` |
| `version_is_still_not_offered`           | no tokens                          | `--version` not among the offerable flags                   |
| `unknown_value_types_are_not_validated`  | String and i64 args                | `value_kind` is `None`; `accepts_value` rejects it          |

Plus merge tests in `dispatch.rs` (existing test module, or one added) calling `merge_root_flags` directly:

- `merge_offers_unseen_non_conflicting_flags`: `rest == ["--"]`, `seen == ["quiet"]`: values are exactly
  `--no-output-timeout-secs --no-timestamps --timeout-secs --timestamps` (engine `--help` pre-seeded in candidates stays).
- `merge_drops_help_once_given`: `seen == ["help"]`, candidates holding `--help`: result lacks `--help`.
- `merge_is_inert_after_a_committed_word`: `rest == ["--quiet-ish", "--"]` or `["--", "--"]`: candidates unchanged.

## Smoke tests (`crates/toolr/tests/complete_smoke.rs`, fixture `ci hello`/`deploy`, helper `complete`)

Each splits stdout into sorted lines and compares.

| Test                                          | Tokens                          | Assertion                                                                  |
| --------------------------------------------- | ------------------------------- | -------------------------------------------------------------------------- |
| `completes_groups_after_a_root_flag`          | `--quiet ""`                    | lines contain `ci`, `project`, `self`; none start with `--`                |
| `completes_group_prefix_after_a_root_flag`    | `--quiet c`                     | `== ["ci"]`                                                                |
| `completes_commands_after_a_root_flag`        | `--quiet ci ""`                 | contains `hello`, `deploy`                                                 |
| `completes_further_root_flags`                | `--quiet --`                    | `== [--help, --no-output-timeout-secs, --no-timestamps, --timeout-secs, --timestamps]` |
| `skips_conflicting_and_given_root_flags`      | `--quiet --timestamps --`       | `== [--help, --no-output-timeout-secs, --timeout-secs]`                    |
| `skips_the_conflict_of_a_given_alias`         | `--ts --no`                     | `== [--no-output-timeout-secs]`                                            |
| `counts_short_and_alias_flags_as_given`       | `-q --`; `--timeout 5 --t`      | first lacks `--quiet` and `--debug`; second `== [--timestamps]`            |
| `completes_after_a_flag_value`                | `--timeout-secs 5 ""`; `--timeout-secs=5 --` | first contains `ci`; second lacks `--timeout-secs`            |
| `completes_nothing_where_clap_rejects_root_flags` | `--quiet --debug --`; `--quiet --quiet --`; `-qd ""`; `--timeout-secs --`; `--timeout-secs ""`; `--timeout-secs abc ""` | each stdout empty, status success |
| `root_flags_stay_root_only`                   | `--quiet ci --`                 | `== [--help]` (engine only, no root flags)                                 |
| `help_given_is_not_offered_again`             | `--help --`                     | lacks `--help`, contains `--quiet`                                         |
| `terminator_blocks_root_flags`   | `-- --`; `--quiet -- --`        | each stdout empty                                                          |

Before/after: run each smoke test against the unfixed binary first; all but the "nothing" rows must fail
(`--quiet --debug --` already passes, which is why it stays a regression guard). Re-check any "rejected" row with
`TOOLR_TRUST_ALL=1 toolr <line> self completion print bash`.

## Verification

- `cargo test -p toolr --lib builtin_completions`, `cargo test -p toolr --test complete_smoke` in the worktree.
- `mise run test`; `prek run --all-files`.
- Patch coverage via `cargo xtask patch-coverage --base main` on a local `cargo llvm-cov` lcov;
  every branch of `scan_root_flags` has a test row above.
- Manual: `toolr __complete "$PWD" --quiet ""` lists groups; a shell `toolr --quiet <TAB>` in bash, zsh, fish.

## Review changes

- Added attached negative values, `--version` / `-V` ending the line, and value-type checking (unit tests above;
  smoke tests `completes_after_an_attached_negative_value` and `stays_empty_after_version_or_bad_values`).
- The specs are derived once and carried on `RootScan`; `Seen`, `scan_long` and `scan_cluster` split the scan.
- `merge_root_flags` takes the scan alone.
