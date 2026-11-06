# Execute timeout test flakes (FU-42)

## Symptom

Under parallel load, tests in `crates/toolr-core/src/command/command_test.rs` fail intermittently
and pass alone: `timeout_kills_the_shells_child`, `timeout_returns_when_an_escaped_process_holds_the_pipe`,
`no_output_timeout_holds_when_a_background_sleep_keeps_pipe_open` (and its `sleep 301` sibling).

## Root causes (both test-only; the product kill and timeout path behaved correctly)

1. **Cross-process cleanup collision.** The tests find their `sleep` by `pgrep -f '^sleep 999$'` and
   `KillMatching` SIGKILLs every match when the test ends. `pgrep` is machine-wide, so a second
   `cargo test` run (another worktree, `cargo llvm-cov`, a second agent) kills this run's `sleep`
   mid-test. `sh -c 'sleep 999; true'` then exits 0 early and the test sees `Ok(0)` instead of a
   timeout. Captured: `DIAG res=Ok(0) sleeper=[pid] seen_after=65ms total=1.5s`, with no kill of that
   pid from this process.
2. **Writer start-up outruns the timeout.** The escaped-process test reads the writer's pid once, right
   after `timeout_secs = 2` plus the 0.5s grace. Under load `sh -c 'echo $$; ...'` can start later than
   that: the capture was empty at 2.5s and held the pid at 7.5s.

## Fix

- Make every `sleep` command line unique per test process (`sleep 999.<pid>`) so neither `pgrep` nor
  `KillMatching` can touch another run's processes.
- Raise the escaped-process test's timeout from 2s to 10s: the observed start-up stall was just over
  2.5s, so 10s leaves roughly 4x headroom. The assertions still hold at any value; only the budget
  changes, and it is the one place where the cause really is a budget that is too small.
