# Signal tests ignore an inherited SIG_IGN: implementation plan

**Spec:** `specs/2026-11-06-sigint-test-inherited-ignore-design.md`

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

## Task 2: archive the spec

- [ ] `git mv` both spec files to `specs/archive/2026/`, then commit.
