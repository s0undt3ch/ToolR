# Signal tests ignore an inherited SIG_IGN

## Problem

`execute::signals::tests::pending_sigint_reaches_the_child` fails now and then on local runs, but
never in CI. When it fails, the `sleep 10` child exits 0 after ten seconds rather than dying of the
forwarded SIGINT.

A non-interactive shell sets SIGINT and SIGQUIT to `SIG_IGN` for a command it starts with `&`. A
test run started as `( cargo llvm-cov … ) &` therefore runs with SIGINT ignored. An ignored signal
stays ignored across `exec`, so the `sleep` child ignores the SIGINT the test sends. You can
reproduce it every time with:

```sh
(trap '' INT; cargo test -p toolr-core --lib execute::signals::tests::pending_sigint)
```

CI never starts the tests with `&`, which is why it never fails there.

## Scope

This is a test bug only. In production, `spawn_runner` starts the runner before `wait_with_signals`
installs its handlers (`crates/toolr/src/dispatch.rs`), so a runner started from a script's
background job keeps the ignore it inherited. That is the behaviour POSIX shells intend, and this
change leaves it alone.

## Design

`sleeper_status` resets SIGINT and SIGTERM to `SIG_DFL` in the child before `exec`, using
`CommandExt::pre_exec`. `signal()` is async-signal-safe, so it is safe to call in that closure.
`Command::spawn` returns only after `exec` succeeds, so the child has default dispositions before
`poll_child` sends anything.

`tests/e2e/test_signals.py` has the same weakness: the `toolr` it starts inherits the ignore and
passes it on to the runner, so the two SIGINT tests see exit 0. Its `Popen` (`preexec_fn`) and its
`pty.fork` child reset SIGINT before `exec` in the same way.

## Testing

- Run the reproduction above. Both `pending_*_reaches_the_child` tests must pass.
- Run the module tests without the trap. Both tests must pass.
- Run `(trap '' INT; uv run pytest tests/e2e/test_signals.py)`. It must pass, as it does without
  the trap.
