# Execute timeout test flakes plan

1. Reproduce: a stress script (local only, not committed) runs `command::` tests in a loop beside CPU
   hogs (foreground children of one Python process). Record failure counts before the fix.
2. Give `KillMatching`, `wait_for_pids` and the `sleep` callers a per-process unique command line.
3. Escape the `.` in the `pgrep` pattern built from that command line.
4. Move the escaped writer's pid into an atomically written file, quoted in the `sh -c` script, and poll
   it.
5. Raise the escaped-process test timeout to 10s and its elapsed bound to match.
6. Prove the collision with two concurrent old binaries (one fails) and two new ones (both pass).
7. Re-run the stress loop; compare failure counts.
8. `cargo test -p toolr-core`, clippy, `prek run --all-files`.
9. Archive both specs as the last commit.
