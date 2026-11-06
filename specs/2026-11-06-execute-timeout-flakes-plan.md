# Execute timeout test flakes plan

1. Reproduce: a stress script runs `command::` tests in a loop beside CPU hogs (foreground children of
   one Python process). Record failure counts before the fix.
2. Give `KillMatching`, `wait_for_pids` and the `sleep` callers a per-process unique command line.
3. Raise the escaped-process test timeout to 10s and its elapsed bound to match.
4. Re-run the stress loop; compare failure counts.
5. `cargo test -p toolr-core`, clippy, `prek run --all-files`.
6. Archive both specs as the last commit.
