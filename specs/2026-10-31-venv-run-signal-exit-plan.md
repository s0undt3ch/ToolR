# Signal Exit Codes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give a signal-killed or out-of-range child the same exit code on every toolr path: `128 + signal`, and never 0
for a failure.

**Architecture:** A new `toolr_core::execute::exit_status` module classifies an `ExitStatus` once and renders it as an
`i32` (for `ctx.run`) or a `u8` (for toolr's own exit). Dispatch, `venv run`, `venv shell` and `ctx.run` all call it. No
signal-handling change.

**Tech Stack:** Rust (toolr-core, toolr), pyo3 extension, pytest, `assert_cmd` integration tests.

**Spec:** [`specs/2026-10-31-venv-run-signal-exit-design.md`](./2026-10-31-venv-run-signal-exit-design.md)

## Global Constraints

- `exit_code`: exit code if present; else `128 + signal` on Unix; else `1`. Windows codes pass through unchanged.
- `process_exit_code`: `exit_code` if it fits in `0..=255`, else `1`. Never 0 for a failing child.
- No change to `RUNNER_SCHEMA_VERSION`, Python `SCHEMA_VERSION` or the runner JSON spec.
- Don't touch signal forwarding (`crates/toolr-core/src/execute/signals.rs`) or switch `venv run` to
  `wait_with_signals`; that belongs to `fix/sigint-forwarding` and a follow-up.
- Conventional Commits. No `Co-Authored-By` footer. Stage files by path; never `git add -A` (the repo has a local-only
  `audit/` dir).
- Format only touched Rust files (`rustfmt <file>`), never `cargo fmt -p`.

## Review Focus

1. **Windows crash status** (`0xC0000005`, negative as `i32`) must make toolr exit 1, not 0. Task 1,
   `process_exit_code_never_reports_success_for_windows_crash` (`#[cfg(windows)]`, runs in the Windows CI job).
2. **Exit code 256 on Windows** must become 1, not 0 or 255. Task 1, `process_exit_code_maps_out_of_range_to_one`.
3. **SIGKILL** (OOM killer) must give 137, not 1. Task 1, `signal_death_maps_to_128_plus_signal`.
4. **`ctx.exit(result.returncode)`** after a SIGTERM'd child must make the command exit 143. Task 2,
   `test_signal_killed_child_returncode`, asserts the value the idiom relies on.
5. **Ordinary non-zero exit** through `venv run` must stay unchanged (3 stays 3). Covered by the existing
   `no_sync_fresh_passes_exit_code_through`; it must stay green.

---

### Task 1: `exit_status` helper and the binary's call sites

**Files:**

- Create: `crates/toolr-core/src/execute/exit_status.rs`
- Modify: `crates/toolr-core/src/execute/mod.rs` (add `pub mod exit_status;`)
- Modify: `crates/toolr/src/dispatch.rs` (end of `run_dispatch`: the `status.code().unwrap_or_else(...)` block and the
  `clamp`)
- Modify: `crates/toolr/src/project.rs` (`venv_shell` and `run_command_in_venv`: `status.code().unwrap_or(1) as u8`)
- Test: `mod tests` in `exit_status.rs`; `crates/toolr/tests/project_venv_run.rs`

**Interfaces:**

- Produces: `pub fn exit_code(status: std::process::ExitStatus) -> i32`
- Produces: `pub fn process_exit_code(status: std::process::ExitStatus) -> u8`
- [ ] **Step 1: Write the failing unit tests** in `exit_status.rs`, building statuses with
      `std::os::unix::process::ExitStatusExt::from_raw` (Unix: raw wait status) or the Windows
      `ExitStatusExt::from_raw(u32)`:

| Test | cfg | Input | Assert |
|---|---|---|---|
| `exit_code_passes_normal_exit_through` | unix | `from_raw(3 << 8)` | `exit_code == 3`, `process_exit_code == 3` |
| `signal_death_maps_to_128_plus_signal` | unix | `from_raw(libc::SIGTERM)`, `from_raw(libc::SIGKILL)` | `exit_code` 143 and 137; `process_exit_code` 143 and 137 |
| `process_exit_code_never_reports_success_for_windows_crash` | windows | `from_raw(0xC000_0005)` | `exit_code == 0xC000_0005_u32 as i32`; `process_exit_code == 1` |
| `process_exit_code_maps_out_of_range_to_one` | windows | `from_raw(256)` | `exit_code == 256`; `process_exit_code == 1` |

- [ ] **Step 2: Write the failing integration test** in `project_venv_run.rs`, next to
  `no_sync_fresh_passes_exit_code_through` and built the same way (`fake_in_tree_venv`, `mark_fresh`,
  `project venv run --no-sync -- python`):

```rust
#[cfg(unix)]
#[test]
fn no_sync_signal_killed_child_exits_128_plus_signal() // python body "#!/bin/sh\nkill -TERM $$\n"
// assert_eq!(output.status.code(), Some(143), "stderr: ...")
```

`venv shell` gets no integration test: it always goes through `ensure_venv_ready`, which needs uv. Its mapping is the
same `process_exit_code` call the unit tests cover.

- [ ] **Step 3: Run the tests and confirm they fail**

Run: `cargo test -p toolr-core --lib execute::exit_status` and
`cargo test -p toolr --test project_venv_run no_sync_signal_killed_child_exits_128_plus_signal`
Expected: compile failure (module missing), then the integration test fails with `Some(1)`.

- [ ] **Step 4: Implement `exit_code` and `process_exit_code`**

`exit_code`: `status.code()`, else on `#[cfg(unix)]` `status.signal().map(|s| 128 + s)`, else `1`.
`process_exit_code`: `u8::try_from(exit_code(status)).unwrap_or(1)`. Module doc comment: one line saying this is the
only place that maps a child status to an exit code.

- [ ] **Step 5: Replace the three binary call sites**

`dispatch.rs` and both `project.rs` sites become `Ok(ExitCode::from(process_exit_code(status)))`. Delete the inline
signal block and the clamp comment in `dispatch.rs`.

- [ ] **Step 6: Run the tests and confirm they pass**

Run: the two commands from Step 3, then `cargo test -p toolr --test project_venv_run`.
Expected: PASS, including `no_sync_fresh_passes_exit_code_through`.

- [ ] **Step 7: Commit**

```bash
git add crates/toolr-core/src/execute/exit_status.rs crates/toolr-core/src/execute/mod.rs \
    crates/toolr/src/dispatch.rs crates/toolr/src/project.rs crates/toolr/tests/project_venv_run.rs
git commit -m "fix(venv): report 128 + signal for a signal-killed child"
```

### Task 2: `ctx.run` returns `128 + signal`

**Files:**

- Modify: `crates/toolr-core/src/command.rs` (`run_command_internal`, `status.code().unwrap_or(-1)`)
- Modify: `crates/toolr-py/python/toolr/utils/command.py` (`run` docstring: describe `returncode`)
- Test: `tests/utils/command/test_command.py`

**Interfaces:**

- Consumes: `toolr_core::execute::exit_status::exit_code` (Task 1), via `crate::execute::exit_status::exit_code`.

- [ ] **Step 1: Write the failing test**

```python
@pytest.mark.skipif(IS_WINDOWS, reason="POSIX signals")
def test_signal_killed_child_returncode():
    result = run(["sh", "-c", "kill -TERM $$"])
    assert result.returncode == 143
```

- [ ] **Step 2: Run it and confirm it fails**

Run: `uv run pytest tests/utils/command/test_command.py::test_signal_killed_child_returncode -v`
Expected: FAIL, `-1 != 143`. Rebuild the extension first (`uv sync --all-extras --dev` reinstalls the local `toolr-py`).

- [ ] **Step 3: Implement**

`let returncode = exit_code(status);`. In the `run` docstring, say `returncode` is `128 + N` when signal `N` killed the
child, as in a shell.

- [ ] **Step 4: Run it and confirm it passes**

Run: the Step 2 command, then `uv run pytest tests/utils/command`.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/command.rs crates/toolr-py/python/toolr/utils/command.py tests/utils/command/test_command.py
git commit -m "fix(run): return 128 + signal from ctx.run for a signal-killed child"
```

### Task 3: Docs, release note, verification, archive

**Files:**

- Modify: `UNRELEASED.md`, `docs/cli.md`, `docs/writing-commands/context.md`, `skills/toolr-command-authoring/SKILL.md`

- Move: both spec files to `specs/archive/2026/`

- [ ] **Step 1: Write the docs**
    - `UNRELEASED.md`: append the spec's text verbatim.
    - `docs/cli.md`, `venv run` section, after "pass straight through": "A child killed by a signal exits with 128 plus
      the signal number, as in a shell."
    - `docs/writing-commands/context.md`, after "Inspect `result.returncode` yourself.": "A child killed by a signal has
      `returncode` 128 plus the signal number (143 for SIGTERM), so `ctx.exit(result.returncode, ...)` passes it through
      unchanged."
    - `SKILL.md`, the "Calling subprocesses" bullet: add "a signal-killed child has `returncode` 128 + N".

- [ ] **Step 2: Verify**

Run: `cargo xtask build-skill-refs --check` (regenerate and commit if the docstring change drifted
`references/commands.md`), then `mise run test` in the background, polling its output every 30–60 s, then
`prek run --all-files` and `uv run mkdocs build --strict`.
Expected: all green.

- [ ] **Step 3: Commit docs**

```bash
git add UNRELEASED.md docs/cli.md docs/writing-commands/context.md skills/toolr-command-authoring/SKILL.md
git commit -m "docs: document exit codes for signal-killed children"
```

- [ ] **Step 4: Archive the specs (final commit)**

```bash
git mv specs/2026-10-31-venv-run-signal-exit-design.md specs/2026-10-31-venv-run-signal-exit-plan.md specs/archive/2026/
git commit -m "docs(specs): archive venv-run-signal-exit spec and plan"
```
