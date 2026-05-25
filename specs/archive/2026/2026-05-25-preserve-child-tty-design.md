# Preserve the child's TTY semantics for stderr

**Date:** 2026-05-25
**Status:** design

## Problem

Every `toolr <user-command>` invocation pipes the Python subprocess's
stderr through a background pumper thread in
`spawn_runner_capturing_stderr`
(`crates/toolr-core/src/execute/spawn.rs`). The pumper buffers the
entire stderr stream until the child exits, then `dispatch.rs` post-mortem
greps the bytes for `ImportError` to substitute a friendlier
"missing dependency" report.

As a side-effect, **the child's fd 2 is a pipe for the lifetime of the
process, not a TTY**. Inside Python:

- `sys.stderr.isatty()` returns `False`
- Rich's `Console(stderr=True).is_terminal` is `False`
- Rich strips `[link=URL]text[/link]` markup → OSC 8 hyperlinks die
- Rich emits no ANSI color codes on stderr unless `force_terminal=True`
  is set somewhere
- Any non-Rich library checking `isatty(2)` (progress bars, pagers,
  password prompts that fall back to stderr) silently degrades

This is also why stderr output appears in a burst **after**
`wait_with_signals` returns rather than streaming live — the pumper
buffers everything until the child exits.

A concrete, repeatable symptom: in a Python toolr command that prints a
URL using `ctx.info("Logz.io link:", f"[link={url}]here[/link]")`,
the user sees the bare URL instead of a clickable `here`, because
Rich's terminal detection on the (piped) stderr returns False and the
markup gets stripped. Downstream code in the user's repo has had to
work around this with raw OSC 8 escape sequences and
`sys.stdin.isatty()` checks, which is a smell — the workaround
shouldn't exist.

The sibling function `spawn_runner` already does the right thing
(`Stdio::inherit()` on all three fds) and even carries the comment
*"so Rich's TTY detection, tools that read stdin, etc., all work."*
It is unused on the dispatch hot path.

## Why the capture exists

`dispatch.rs:206-220` (paraphrased):

```rust
let (mut child, stderr_capture) =
    spawn_runner_capturing_stderr(&python, tempfile.path())?;
let status = wait_with_signals(&mut child)?;
let stderr_bytes = stderr_capture.take();
let stderr_str = String::from_utf8_lossy(&stderr_bytes);
if !status.success() {
    if let Some(report) = toolr_core::deps_check::intercept_import_error(&stderr_str) {
        std::io::stderr().write_all(report.render().as_bytes())?;
    } else {
        std::io::stderr().write_all(&stderr_bytes)?;
    }
} else {
    std::io::stderr().write_all(&stderr_bytes)?;
}
```

The capture buys exactly one thing: when the Python runner fails to
import a module at startup, toolr can rewrite the raw traceback into
the structured "you're missing dep X" report
(`deps_check::intercept_import_error`).

That said, there is already a **preflight** at `dispatch.rs:182-191`
that calls `deps_check::check_imports(&site_packages, &cmd.imports)`
against the declared `cmd.imports` list before spawning. So the
post-mortem capture is the **belt** to the preflight's **suspenders**:
it only catches imports that the command's manifest entry **failed to
declare**.

The preflight can be disabled with `TOOLR_NO_PREFLIGHT_DEPS=1`. When
it is disabled, the post-mortem capture is the only safety net.

## Goals

- The child's fd 2 is a real TTY when the user is at one — so Rich's
  stderr console detects it, OSC 8 hyperlinks render, colors render,
  and `isatty(2)` is `True` inside Python.
- The child's fd 2 streams to the real stderr in real time, not in a
  burst at the end of execution.
- The friendly `ImportError` report from `intercept_import_error`
  remains available for the failure mode it was designed for: an
  undeclared import that crashes the runner before any user output.
- No regression for non-interactive contexts (CI, redirected stderr,
  piped to another command). When the user's stderr already isn't a
  TTY, behavior is unchanged.
- Windows is not regressed. If the chosen mechanism is Unix-only, the
  Windows path either gets a documented degradation or a separate
  implementation.

## Non-goals

- Capturing stdout. Stdout is already inherited and works correctly
  today. We do not touch it.
- Solving "two consoles in different streams should reach a real
  terminal each, even when one of them is captured." Stdout stays
  inherited; only stderr is in scope.
- Replacing `deps_check::intercept_import_error`'s rendering. The
  report itself is unchanged; the only question is **whether** and
  **when** to invoke it.
- Bringing back live stderr through the pipe (option 2 below) without
  also fixing the TTY classification — the latter is the load-bearing
  half.

## Options

### Option A — Drop the capture, lean on the preflight (~5 LOC)

In `dispatch.rs`, replace the `spawn_runner_capturing_stderr` call
with `spawn_runner`. Delete the `stderr_capture.take()` /
`intercept_import_error` branch. Stderr inherits → child sees a TTY →
Rich works.

**Coverage tradeoff:** an undeclared import that fails at runner
start-up surfaces as the raw Python traceback the user would see
anywhere else, not as the styled "missing dep" report. The preflight
still catches every **declared** missing dep at line 185.

**Cost:** ~5 lines removed, no new code.

**Risk:** plugin authors who forget to declare a transitive dep get a
worse error message than today. Mitigation: document the requirement,
keep `intercept_import_error` reachable from a debugging command if
needed, optionally call it on demand from a `--explain-last-error`
flag.

### Option B — Tee stderr live, still capture for post-mortem (~15 LOC)

Modify the pumper in `spawn_runner_capturing_stderr` to write each
chunk to `io::stderr()` **as well as** appending to the in-memory
buffer. Stderr appears in real time; the post-mortem still works.

**What this fixes:** the latency bug (stderr appearing in a burst).

**What this does not fix:** the TTY classification. Python's fd 2 is
still a pipe, Rich still detects non-terminal, OSC 8 hyperlinks
still die. **This option does not solve the actual problem the spec
is about.** Listed here only so it's explicit that "fix the latency"
and "fix the TTY" are different problems.

**Recommendation:** skip unless you specifically want streaming
without the TTY fix.

### Option C — PTY for stderr only (~80-120 LOC Unix, separate Windows path)

Add a new spawn variant: `spawn_runner_with_stderr_pty`. It opens
**one** PTY pair (using `nix::pty::openpty` or `portable-pty`), sets
the child's fd 2 to the PTY slave, and inherits fd 0 and fd 1. The
parent runs a pumper that:

1. Reads from the PTY master
2. Writes each chunk to `io::stderr()` (live forwarding)
3. Optionally appends to a bounded buffer for `intercept_import_error`

The PTY master is the parent's only handle to the child's stderr,
so the child sees a TTY for fd 2 → Rich detects it → OSC 8 works.

**Extras to budget for:**

- `setsid()` + `ioctl(TIOCSCTTY)` in the child for proper signal
  delivery (most ergonomic via `portable-pty`'s `CommandBuilder` if
  the manual fork is undesirable, but `CommandBuilder` assumes a
  single PTY for all three fds — for stderr-only you drop to `nix`)
- `SIGWINCH` handler that resizes the PTY when the user's terminal
  resizes, so Rich's stderr-side width matches reality
- An ANSI-stripping pass on the post-mortem buffer if
  `intercept_import_error`'s regexes are sensitive to escape codes
  (they currently aren't, but Rich-formatted tracebacks contain
  them)
- A bounded buffer (e.g. 256 KiB ring) instead of unbounded
  accumulation, since live-forwarded stderr can be arbitrarily large

**Windows:** ConPTY doesn't cleanly attach to a single fd of a
process. The realistic Windows path is `spawn_runner` (inherit)
unconditionally, accepting that the post-mortem report doesn't fire
there. Gate the new code with `#[cfg(unix)]` and keep
`spawn_runner_capturing_stderr` for `#[cfg(windows)]`, or pick
option A unconditionally and skip the platform split.

### Option D — Default to inherit, opt into capture (~10 LOC)

Keep both spawn functions. `dispatch.rs` chooses between them based
on the same signal as the preflight:

```rust
let want_capture = std::env::var_os("TOOLR_NO_PREFLIGHT_DEPS")
    .is_some_and(|v| !v.is_empty() && v != "0");

if want_capture {
    spawn_runner_capturing_stderr(...)  // preflight off → use safety net
} else {
    spawn_runner(...)                    // preflight on → inherit
}
```

99% of runs get inherit semantics and a real TTY. The 1% that explicitly
disable the preflight pay for the capture and get the friendly report.

**Cost:** ~10 lines in `dispatch.rs` plus a branch on the result
type. The `wait_with_signals` / exit-code mapping is identical between
branches; only the `stderr_capture.take()` + interception block becomes
conditional.

## Recommendation

**Option A** if you're willing to make manifest declaration the
authoritative source of truth for the runner's dependencies. The
preflight already does the job; the post-mortem is a fallback for a
case that documentation and good defaults should make rare.

**Option D** if you want a zero-regression path. The current `dispatch.rs`
behavior is preserved when `TOOLR_NO_PREFLIGHT_DEPS=1` is set, and the
default path becomes `spawn_runner` (inherit). Users who don't reach
for the env var get the better terminal experience automatically.

**Option C** is correct engineering but spends ~100 lines + platform
complexity on a use case the preflight already covers. Reasonable if
toolr's design philosophy includes "the user can always disable the
preflight and we still give them great errors," not reasonable
otherwise.

**Option B** alone doesn't solve the stated problem; only consider as
a complement to C if you want to keep "live forwarding" semantics
while also adding the PTY.

## Open questions for the implementation session

1. Is the friendly `intercept_import_error` report load-bearing for
   any documented user-facing flow today? If yes, option A is harder
   to justify and option D becomes the floor.
2. What does the test matrix look like for `dispatch_command()`?
   The current tests appear to be unit-level around manifest
   resolution; an end-to-end test that asserts on TTY-vs-pipe
   semantics will likely need a PTY harness (`portable-pty` as a
   dev-dependency).
3. Should `spawn_runner_capturing_stderr` be retained in the public
   surface of `toolr-core::execute` after the change, or moved to a
   `#[cfg(test)]`-gated helper / removed entirely? Removing it
   simplifies the API; keeping it preserves the building block for
   other capture use cases.
4. Is there appetite for adding a `--explain-last-error` subcommand
   that re-runs `intercept_import_error` against the last failed
   stderr (cached somewhere)? Would let option A keep the friendly
   report as an opt-in without it being on the hot path.
5. On Windows, do we accept "no friendly ImportError report" or
   build a parallel `spawn_runner_capturing_stderr_windows`? Today's
   Windows behavior matches the captured path; switching to inherit
   loses parity with macOS/Linux.

## Files touched (estimate)

- `crates/toolr/src/dispatch.rs` — call site change, branch on
  preflight env if Option D
- `crates/toolr-core/src/execute/spawn.rs` — optional new variant
  if Option C
- `crates/toolr-core/src/execute/mod.rs` — re-export the new
  variant if Option C
- `Cargo.toml` (crate-level) — add `nix` or `portable-pty` as a
  Unix-only dependency if Option C
- Tests under `crates/toolr-core/src/execute/` and
  `crates/toolr/tests/` — TTY-vs-pipe assertion, exit-code mapping
  unchanged, ImportError report still rendered for the
  preflight-off branch under Option D

## Repro

In a tools project:

```python
# tools/example.py
from toolr import Context, command_group

group = command_group("example", "Demo")

@group.command
def hyperlink(ctx: Context) -> None:
    url = "https://example.com/some/long/path"
    ctx.info(f"Click: [link={url}]here[/link]")
```

Run `toolr example hyperlink` at an interactive terminal that
supports OSC 8 (iTerm2, Ghostty, Kitty, WezTerm, Windows Terminal).

**Today (with capture):** terminal shows `Click: here` with no link
target — the `[link=URL]` part was stripped because Rich saw fd 2 as
a pipe.

**After fix:** terminal shows `Click: here` where `here` is a clickable
hyperlink to the URL.

Verify the failure mode by piping: `toolr example hyperlink 2>&1 | cat`
should show `Click: here` in both cases (no terminal to click in
anyway).
