# Signal tests ignore an inherited SIG_IGN: implementation plan

**Spec:** `specs/archive/2026/2026-11-06-sigint-test-inherited-ignore-design.md`

## Task 1: reset dispositions in the sleeper child

**File:** `crates/toolr-core/src/execute/signals.rs`, `mod tests`, `sleeper_status`.

- [ ] Run the reproduction from the spec and confirm that `pending_sigint_reaches_the_child` fails.
- [ ] Add a `pre_exec` closure to the `sleep` command. In it, call `libc::signal(SIGINT, SIG_DFL)`
      and `libc::signal(SIGTERM, SIG_DFL)`, then return `Ok(())`. Add a `// SAFETY:` comment that
      says `signal` is async-signal-safe. Update the helper's doc comment to say why the reset
      is there.
- [ ] Run the reproduction again and confirm that both `pending_*` tests pass.
- [ ] Run `cargo test -p toolr-core --lib execute::signals` and confirm that it passes.
- [ ] Commit `fix(execute): reset inherited signal dispositions in the signal tests` with a
      `Release-Notes: skip` trailer, because nothing user-visible changes.

## Task 2: reset SIGINT in the e2e signal tests

**File:** `tests/e2e/test_signals.py`.

- [ ] Run `(trap '' INT; uv run pytest tests/e2e/test_signals.py)` and confirm that the two SIGINT
      tests fail with exit 0.
- [ ] Add `reset_sigint()`, which sets SIGINT to `SIG_DFL`. Call it through `preexec_fn` on the
      `Popen` and in the `pty.fork` child before `execve`.
- [ ] Run the same command again and confirm that it passes.

## Task 3: archive the spec

- [ ] `git mv` both spec files to `specs/archive/2026/`, then commit.
