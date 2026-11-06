# Plan: root `--quiet` in `self cache prune`

Design: [`2026-11-06-quiet-cache-prune-design.md`](2026-11-06-quiet-cache-prune-design.md). Stacked on PR #695
(`fix/root-quiet-builtins`); create the branch with `git-spice branch create`. Part 1 only is planned in detail;
the Part 2 task depends on the user's answer to open question 1.

## Task 1: thread `quiet` and add `Report` (red first)

Files: `crates/toolr/src/dispatch.rs`, `crates/toolr/src/self_cache.rs`, `crates/toolr/src/self_cache_prune.rs`.

1. Write the unit and integration tests below; they fail (no `quiet` plumbing, no `Report`).
2. `run_self`: `Some(("cache", m)) => crate::self_cache::dispatch(m, quiet)`. `self_cache::dispatch(matches, quiet)`
   passes it to `self_cache_prune::run(&root, prune_m, quiet)`; `list` ignores it.
3. Add `Report` (`note` / `result` / `warn`) and replace the `out` parameter of `prune_all`, `prune_classified`,
   `sweep_locks`, `remove_entry`. Classify every existing `writeln!` per the design table. `note` is silent when
   `quiet && !dry_run`. Keep `sweep_locks`' returned count independent of printing.
4. `prune_all` takes `confirm: impl FnOnce(&[CachedVenv]) -> Result<bool>`; `run` passes `confirm_destroy_all`.
5. Format touched files only (`rustfmt --edition 2024 <file>`).

## Unit tests (`self_cache_prune.rs`, `#[cfg(test)]`)

Use the existing `make_entry` and a `Vec<u8>` buffer.

- `prune_all_quiet_real_run_removes_silently`: quiet, `yes`, two entries on disk; buffer empty, both dirs gone.
- `prune_all_quiet_dry_run_keeps_result_lines`: buffer contains `DRY-RUN would remove` twice and `would free`;
  dirs still exist.
- `prune_all_quiet_empty_is_silent`: empty entries, no locks; buffer empty.
- `prune_all_quiet_dry_run_empty_still_reports`: same with `dry_run`; buffer contains `no cache entries to remove`.
- `prune_all_quiet_declined_prompt_is_silent_and_keeps_entries`: `confirm` returns `Ok(false)`; buffer empty, dirs exist.
- `prune_all_declined_prompt_reports_abort`: not quiet; buffer contains `aborted, nothing removed`.
- `prune_classified_quiet_real_run_removes_silently`: orphan candidate; buffer empty (no `removing`, no `freed`),
  dir gone.
- `prune_classified_quiet_dry_run_keeps_tagged_lines`: buffer contains `DRY-RUN ORPHAN` and `would free`.
- `prune_classified_quiet_nothing_to_prune_is_silent`, and `..._dry_run_nothing_to_prune_still_reports`
  (`nothing to prune` present).
- `sweep_locks_quiet_returns_count_without_printing`: one stray lock; returns `1`, buffer empty, lock removed.
  This guards the "nothing to prune" fallback.
- `sweep_locks_quiet_dry_run_still_reports`: returns `1`, buffer contains `would remove 1 rebuild lock`, lock kept.
- `remove_entry_quiet_still_warns`: a missing directory; buffer contains `warning: failed to remove`. Pins the
  warning side, and is the test to flip if option (b) is chosen.

## Integration tests (`tests/self_cache_prune.rs`)

Add `run_prune_root(xdg, root_flag, args) -> Output` (puts the flag before `self cache prune`, asserts exit status
as `run_prune` does). Reuse `write_entry` and `cache_with_kept_and_stray_lock`. Each success case is parametrised
over `-q` and `--quiet`.

- `root_quiet_prune_is_silent`: orphan entries; stdout and stderr both empty, orphan dirs gone.
- `root_quiet_prune_dry_run_keeps_lines`: stdout has `DRY-RUN ORPHAN` and `would free`, stderr empty, dirs remain.
- `root_quiet_prune_nothing_to_prune_is_silent`: empty cache; stdout and stderr empty.
- `root_quiet_prune_all_yes_is_silent`: `--all --yes`; stdout and stderr empty, dirs gone.
- `root_quiet_prune_all_refuses_without_yes`: `--all` with null stdin; exit non-zero, stderr contains
  `refusing to wipe`, entries still exist (the refusal is not silenced).
- `root_quiet_prune_lock_sweep_is_silent`: stray lock removed, stdout empty; with `--dry-run` stdout has
  `would remove 1 rebuild lock` and the lock stays.
- `root_quiet_prune_sweep_failure_still_warns`: copy of `prune_warns_and_succeeds_when_lock_sweep_fails` under `-q`;
  stdout contains `warning: failed to sweep rebuild locks`.
- Regression guard: the existing un-quiet tests stay untouched and green.

## Task 2: warning rule, decided (a): warnings print under `-q` everywhere

- Drop `!argv_requests_quiet(&argv)` from the `main.rs` warning gate. Invert `quiet_suppresses_the_warning` into
  `quiet_keeps_the_warning`, and update the `argv_requests_quiet` comment.
- Find any other warning gated on quiet and ungate it, each with a test.
- Reword the root `-q` help in `cli.rs` and the `dispatch_project` doc comment to say warnings stay. Regenerate doc
  snippets (`toolr pre-commit regen-doc-snippets`) and skill refs (`cargo xtask build-skill-refs`) if the help text
  is captured.
- Add a `**Breaking:**` release note: scripts running `toolr -q` now see warnings, such as skipped plugins.

## Task 3: finish

1. `toolr release-notes add --text "…"` (root `-q` now silences `self cache prune`'s success output).
2. `mise run test` (Rust and Python touched), `prek run --all-files`.
3. Coverage: `cargo xtask patch-coverage --base fix/root-quiet-builtins` against the CI lcov (run by CI or the
   user: local llvm-cov is off the table on this disk). Every new branch above has a test.
4. Last commit: `git mv` this design and plan to `specs/archive/2026/`.
