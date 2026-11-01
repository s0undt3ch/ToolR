# SIGINT Forwarding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ctrl-C reaches a toolr command's runner exactly once, so its `finally:` and `except KeyboardInterrupt:` blocks
run to completion.

**Architecture:** `wait_with_signals` keeps SIGINT registered (toolr survives Ctrl-C and reports the runner's exit code)
but forwards SIGTERM only. The `AtomicI32` slot becomes an `AtomicBool`. Two end-to-end tests drive the real binary and
send real signals.

**Tech Stack:** Rust (toolr-core, `signal_hook`, `libc`), pytest end-to-end tests against the built binary.

**Spec:** [`specs/2026-10-31-sigint-forwarding-design.md`](./2026-10-31-sigint-forwarding-design.md)

## Global Constraints

- The runner stays in toolr's process group. No `setsid`, `process_group` or `tcsetpgrp`.
- toolr registers SIGINT and SIGTERM; it forwards SIGTERM only.
- Windows path (`child.wait()`) unchanged. New tests are `skipif(sys.platform == "win32")`.
- No change to `RUNNER_SCHEMA_VERSION`, Python `SCHEMA_VERSION` or the runner JSON spec.
- Don't touch `crates/toolr/src/project.rs` or `dispatch.rs`'s exit-code mapping; `fix/venv-run-signal-exit` owns them.
- Signal tests live in `tests/e2e/`, not in `cargo test`: signal handlers are process-wide and Rust tests share a
  process.
- Conventional Commits. No `Co-Authored-By` footer. Stage files by path; never `git add -A` (the repo has a local-only
  `audit/` dir).
- Format only touched Rust files (`rustfmt <file>`), never `cargo fmt -p`.

## Review Focus

1. **Group SIGINT with no terminal** (CI, a process manager's `killpg`) must reach the runner once. Task 1,
   `test_group_sigint_lets_finally_finish`, uses exactly that delivery.
2. **SIGTERM to toolr alone** must still stop the runner, and toolr must exit 143. Task 1,
   `test_sigterm_to_toolr_is_forwarded`.
3. **A runner that ignores SIGINT** must keep toolr waiting, not orphan it. Follows from "register, don't forward";
   Task 2's unit test `sigint_is_not_forwarded` pins the rule.
4. **SIGINT then SIGTERM inside one 50 ms poll** must still forward the SIGTERM. Task 2,
   `sigterm_survives_a_preceding_sigint`.
5. **A quick child** must still return promptly. Existing `wait_returns_for_quick_child` stays green.

---

### Task 1: Reproduce the double SIGINT (close criteria)

The finding was rated plausible. The design's repro already failed on macOS; this task makes it a test.

**Files:**

- Modify: `tests/conftest.py` (`ToolsProject`: add a `popen` method)
- Create: `tests/e2e/test_signals.py`

**Interfaces:**

- Produces: `ToolsProject.popen(self, *args: str) -> subprocess.Popen[str]`. Same `cwd`/`env` as `run`, with
  `stdout=PIPE`, `stderr=PIPE`, `text=True`, `start_new_session=True` (toolr leads a new process group with no
  controlling terminal).

- [ ] **Step 1: Write the tests.** Module body, in the audit's shape plus a readiness marker:

```python
SIG_MODULE = '''
import time
from toolr import Context, command_group

group = command_group("sig", "Signals", description="Signal tests")


@group.command
def cleanup(ctx: Context) -> None:
    """Sleep, then clean up slowly."""
    print("ready", flush=True)
    try:
        time.sleep(10)
    finally:
        time.sleep(0.5)
        print("cleaned", flush=True)
'''
```

| Test | Action after reading `ready` from `proc.stdout` | Assert (`proc.communicate(timeout=15)`) |
|---|---|---|
| `test_group_sigint_lets_finally_finish` | `os.killpg(proc.pid, signal.SIGINT)` | `"cleaned" in stdout`; `proc.returncode == 130` |
| `test_sigterm_to_toolr_is_forwarded` | `os.kill(proc.pid, signal.SIGTERM)` | `proc.returncode == 143`; `"cleaned" not in stdout` |

Both use `make_tools_project(sig=SIG_MODULE)` and `project.popen("sig", "cleanup")`, and are
`pytest.mark.skipif(sys.platform == "win32", reason="POSIX signals")`.

- [ ] **Step 2: Run against the unchanged code**

Run: `uv run pytest tests/e2e/test_signals.py -v` three times.
Expected: `test_group_sigint_lets_finally_finish` FAILS (no `cleaned`); `test_sigterm_to_toolr_is_forwarded` PASSES.

**Close criteria:** if `test_group_sigint_lets_finally_finish` passes in all three runs on both macOS and Linux, the
finding does not reproduce. Commit the tests as a regression guard (`test(e2e): cover signal delivery to commands`),
skip Task 2, delete both spec files instead of archiving them in Task 3, and stop.

- [ ] **Step 3: Commit**

```bash
git add tests/conftest.py tests/e2e/test_signals.py
git commit -m "test(e2e): reproduce double SIGINT delivery to commands"
```

### Task 2: Forward SIGTERM only

**Files:**

- Modify: `crates/toolr-core/src/execute/signals.rs` (module doc, `unix::wait_with_signals`, `mod tests`)
- Modify: `crates/toolr-py/python/toolr/_runner.py` (comment in `run`'s `except KeyboardInterrupt:`)

**Interfaces:**

- Produces (private to `unix`): `fn should_forward(sig: i32) -> bool`, true only for `SIGTERM`.

- Produces (private): `fn record(pending: &AtomicBool, sig: i32)` sets the flag when `should_forward(sig)`;
  `fn take(pending: &AtomicBool) -> bool` is `swap(false)`.

- [ ] **Step 1: Write the failing unit tests** in `signals.rs` `mod tests` (pure functions, no real signals):
    - `sigint_is_not_forwarded`: `!should_forward(SIGINT)` and `should_forward(SIGTERM)`.
    - `sigterm_survives_a_preceding_sigint`: `record(&p, SIGTERM); record(&p, SIGINT);` then `take(&p)` is `true` and a
      second `take(&p)` is `false`.

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo test -p toolr-core --lib execute::signals`
Expected: compile failure (`should_forward` missing).

- [ ] **Step 3: Implement**

Keep `Signals::new([SIGINT, SIGTERM])` so Ctrl-C does not kill toolr. The listener calls `record`; the polling loop
calls `take` and, when true, `libc::kill(child_pid, SIGTERM)`. Rewrite the module doc to say why SIGINT is caught but
not forwarded (the terminal already delivered it to the foreground group). Update the `_runner.py` comment: SIGINT comes
from the terminal or the process group, not from toolr.

- [ ] **Step 4: Run all signal tests and confirm they pass**

Run: `cargo test -p toolr-core --lib execute::signals`, then `uv run pytest tests/e2e/test_signals.py -v` three times.
Expected: PASS every time.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/execute/signals.rs crates/toolr-py/python/toolr/_runner.py
git commit -m "fix(runner): stop forwarding SIGINT the terminal already delivered"
```

### Task 3: Release note, verification, archive

**Files:**

- Modify: `UNRELEASED.md`

- Move: both spec files to `specs/archive/2026/`

- [ ] **Step 1: Release note.** Append the spec's `UNRELEASED.md` text verbatim.

- [ ] **Step 2: Verify**

Run: `mise run test` in the background, polling its output every 30–60 s, then `prek run --all-files`.
Expected: all green. No docs pages change, so `mkdocs build` is not needed.

- [ ] **Step 3: Commit**

```bash
git add UNRELEASED.md
git commit -m "docs(unreleased): note single SIGINT delivery"
```

- [ ] **Step 4: Archive the specs (final commit)**

```bash
git mv specs/2026-10-31-sigint-forwarding-design.md specs/2026-10-31-sigint-forwarding-plan.md specs/archive/2026/
git commit -m "docs(specs): archive sigint-forwarding spec and plan"
```
