# Global flags and `--quiet`: plan

1. `toolr-core`: `prompt::yes_no` + tests; use it in `trust::confirm` and `project::confirm_sync`.
   `project::NothingToSync` + `is_nothing_to_sync` + tests.
2. `cli.rs`: drop the per-command `quiet` args, add `venv sync --if-project`, keep the root flags root-only
   (`global_flag_args`) and add the usage example to root help. `main.rs`: `misplaced_root_flag_hint`, the
   cache hint from `matches`.
3. `project.rs`: `ready_venv`, `ProjectCtx`, the root `quiet` threaded into every handler,
   `venv_sync` uses `is_nothing_to_sync`; `prompt_overwrite_file` and `confirm_destroy_all` use
   `yes_no_stdin`. `dispatch.rs`: `resync_if_needed` uses `ready_venv`.
4. Tests: `tests/global_flags.rs` (root flags, hint for misplaced and old-hook spellings, no hint when the
   command owns the spelling, parameters named like the flags); `--if-project` in `project_venv_sync.rs`,
   `quiet_uv_consent.rs`, `repo_trust.rs`; `cache_hint_quiet.rs`; pty prompt tests; subcommand-level `-q`
   moved to the root throughout.
5. Docs (`cli.md`, `installation/index.md`, `installation/mise.md`), regenerated snippets and skill refs,
   release note (breaking).
6. `prek run --all-files`, `cargo test -p toolr -p toolr-core`, patch coverage against `origin/main`,
   archive the specs.
