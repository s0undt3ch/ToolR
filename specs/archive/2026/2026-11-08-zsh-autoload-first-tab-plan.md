# Plan: zsh first-Tab autoload

1. Add `zsh_tab_via` to `crates/toolr/tests/complete_shells.rs`, loading the script either sourced or
   as an autoloaded `_toolr` in an `fpath` directory, and a test that presses Tab once in each mode.
   Confirm it fails for the autoloaded mode.
2. End `crates/toolr-core/src/complete/scripts/zsh.zsh` with the `loadautofunc` guard. Confirm both
   modes pass.
3. Assert the guard is present in `zsh_script_invokes_toolr_complete`.
4. Queue a release note.
5. Run the zsh tests, `prek run --all-files` and patch coverage.
6. Archive this spec and plan as the last commit.
