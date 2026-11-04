# Signal Exit Codes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `ctx.run` reports a signal death as `-N`, like `subprocess`; every process exit code toolr produces
(dispatch, `venv run`, `venv shell`, the runner via `ctx.exit` or `SystemExit`) reports it as `128 + N`, like a shell;
an out-of-range code becomes 1 and a failure never becomes 0.

**Architecture:** A new ungated `toolr_core::exit_status` module classifies an `ExitStatus` once and renders it as
`returncode` (`i32`, for `ctx.run`) or `process_exit_code` (`u8`, for toolr's own exit). A Python helper,
`toolr._context.status_to_exit_code`, applies the shell rule in `Context.exit` and in the runner's `SystemExit` branch.
No signal-handling change, no runner schema change.

**Tech Stack:** Rust (toolr-core, toolr), pyo3 extension, pytest, `assert_cmd` integration tests.

**Spec:** [`specs/2026-10-31-venv-run-signal-exit-design.md`](./2026-10-31-venv-run-signal-exit-design.md)

## Before starting

- **Wait for #645 (`fix/ctx-run-timeout-kills-tree`) to merge**, then rebase this branch onto `main`. #645 rewrites
  `crates/toolr-core/src/command.rs` and moves its tests to `crates/toolr-core/src/command/command_test.rs`; Task 2
  targets that layout. Confirm the call site is still `let returncode = status.code().unwrap_or(-1);` at the end of
  `run_command_internal` (`grep -n 'unwrap_or(-1)' crates/toolr-core/src/command.rs`). If #645 moved it, follow it.

## Global Constraints

- `returncode`: exit code if present; else `-signal` on Unix; else `-1`. Windows codes pass through unchanged.
- `process_exit_code`: exit code if it fits in `0..=255`; else `128 + signal` on Unix; else `1`. Never 0 for a failing
  child.
- `status_to_exit_code` (Python): `status < 0` becomes `128 + abs(status)`; then anything outside `0..=255` becomes
  `1`; else unchanged.
- `exit_status.rs` lives at `crates/toolr-core/src/exit_status.rs`, declared `pub mod exit_status;` in `lib.rs`
  **without** `#[cfg(feature = "full")]` (toolr-py builds without `full`, and `command.rs` calls it). `std` only; `libc`
  may appear in its `#[cfg(test)]` module (it is a dev-dependency).
- No change to `RUNNER_SCHEMA_VERSION`, Python `SCHEMA_VERSION` or the runner JSON spec.
- Don't touch signal forwarding (`crates/toolr-core/src/execute/signals.rs`) or switch `venv run` to
  `wait_with_signals`; that is a separate follow-up.
- Conventional Commits. No `Co-Authored-By` footer. Stage files by path; never `git add -A` (the repo has a local-only
  `audit/` dir).
- Format only touched Rust files (`rustfmt <file>`), never `cargo fmt -p`.

## Review Focus

1. **Windows crash status** (`0xC0000005`, negative as `i32`) must make toolr exit 1, not 0, and must reach Python
   unchanged through `ctx.run`. Task 1, `windows_crash_status_passes_through_returncode_and_exits_one`
   (`#[cfg(windows)]`, runs in the Windows CI job).
2. **Exit code 256** must become 1, not 0 or 255, on every process-exit path. Task 1,
   `process_exit_code_maps_out_of_range_to_one` (Windows); Task 3, `test_status_to_exit_code` (`256 -> 1`).
3. **SIGKILL** (OOM killer) must give 137 from `venv run`, not 1. Task 1,
   `process_exit_code_is_128_plus_signal_for_signal_death`.
4. **`ctx.run` parity with `subprocess`.** Task 2, `test_signal_killed_child_returncode` compares against
   `subprocess.run` itself, not a literal.
5. **The idiom `ctx.exit(ctx.run(...).returncode)`** after a SIGTERM'd child must make the runner exit 143. Task 3,
   `test_runner_exits_128_plus_signal_for_signal_killed_ctx_run`, end to end.
6. **Ordinary non-zero exit** through `venv run` stays unchanged (3 stays 3): the existing
   `no_sync_fresh_passes_exit_code_through` must stay green. Through the runner, `test_run_returns_integer_exit_code_from_systemexit`
   (7 stays 7) must stay green.

---

### Task 1: `exit_status` module and the binary's call sites

**Files:**

- Create: `crates/toolr-core/src/exit_status.rs`
- Modify: `crates/toolr-core/src/lib.rs` (add ungated `pub mod exit_status;`)
- Modify: `crates/toolr/src/dispatch.rs` (end of `run_dispatch`: the `status.code().unwrap_or_else(...)` block and the
  `clamp`)
- Modify: `crates/toolr/src/project.rs` (`venv_shell` and `run_command_in_venv`: `status.code().unwrap_or(1) as u8`)
- Test: `#[cfg(test)] mod tests` in `exit_status.rs`; `crates/toolr/tests/project_venv_run.rs`

**Interfaces:**

- Produces: `pub fn returncode(status: std::process::ExitStatus) -> i32`
- Produces: `pub fn process_exit_code(status: std::process::ExitStatus) -> u8`
- [ ] **Step 1: Write the failing unit tests** in `exit_status.rs`. Build statuses with
  `std::os::unix::process::ExitStatusExt::from_raw(i32)` (a raw wait status: `code << 8` for an exit, the signal
  number for a signal death) or `std::os::windows::process::ExitStatusExt::from_raw(u32)`:

| Test | cfg | Input | Assert |
|---|---|---|---|
| `normal_exit_passes_through_both_renderers` | unix | `from_raw(3 << 8)` | `returncode == 3`; `process_exit_code == 3` |
| `returncode_is_negative_signal_for_signal_death` | unix | `from_raw(libc::SIGTERM)`, `from_raw(libc::SIGKILL)` | `returncode` is `-15` and `-9` |
| `process_exit_code_is_128_plus_signal_for_signal_death` | unix | same two statuses | `process_exit_code` is 143 and 137 |
| `windows_crash_status_passes_through_returncode_and_exits_one` | windows | `from_raw(0xC000_0005)` | `returncode == 0xC000_0005_u32 as i32`; `process_exit_code == 1` |
| `process_exit_code_maps_out_of_range_to_one` | windows | `from_raw(256)` | `returncode == 256`; `process_exit_code == 1` |

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

Run: `cargo test -p toolr-core --lib exit_status` and
`cargo test -p toolr --test project_venv_run no_sync_signal_killed_child_exits_128_plus_signal`
Expected: compile failure (module missing), then the integration test fails with `Some(1)`.

- [ ] **Step 4: Implement `returncode` and `process_exit_code`**

`returncode`: `status.code()`, else on `#[cfg(unix)]` `status.signal().map(|s| -s)`, else `-1`.
`process_exit_code`: `status.code()` through `u8::try_from`, else on `#[cfg(unix)]` `status.signal()` as `128 + s`,
else `1`; any `try_from` failure is `1`. Module doc comment: one line saying this is the only place that maps a child
status to a number, and which renderer serves which audience.

- [ ] **Step 5: Replace the three binary call sites**

`dispatch.rs` and both `project.rs` sites become `Ok(ExitCode::from(process_exit_code(status)))`. Delete the inline
signal block and the clamp comment in `dispatch.rs`.

- [ ] **Step 6: Run the tests and confirm they pass**

Run: the two commands from Step 3, then `cargo test -p toolr --test project_venv_run`, then
`cargo build -p toolr-py` (proves the module builds without `full`).
Expected: PASS, including `no_sync_fresh_passes_exit_code_through`.

- [ ] **Step 7: Commit**

```bash
git add crates/toolr-core/src/exit_status.rs crates/toolr-core/src/lib.rs \
    crates/toolr/src/dispatch.rs crates/toolr/src/project.rs crates/toolr/tests/project_venv_run.rs
git commit -m "fix(venv): exit 128 + signal for a signal-killed child, 1 for out-of-range codes"
```

### Task 2: `ctx.run` returns `-N`

**Files:**

- Modify: `crates/toolr-core/src/command.rs` (`run_command_internal`, `status.code().unwrap_or(-1)`)
- Modify: `crates/toolr-py/python/toolr/utils/command.py` (`run` docstring: describe `returncode`)
- Test: `crates/toolr-core/src/command/command_test.rs`; `tests/utils/command/test_command.py`

**Interfaces:**

- Consumes: `crate::exit_status::returncode` (Task 1).

- [ ] **Step 1: Write the failing tests**

In `command_test.rs` (inside `src/`, so it counts towards coverage), next to `test_environment_variables` and built
the same way (`CommandConfig::new(vec!["sh", "-c", "kill -TERM $$"])`, then `run_command_internal(config)`):

```rust
#[cfg(unix)]
#[test]
fn test_signal_killed_child_returncode_is_negative() -> Result<()>
// assert_eq!(run_command_internal(config).map_err(...)?, -libc::SIGTERM);
```

In `test_command.py`:

```python
@pytest.mark.skipif(IS_WINDOWS, reason="POSIX signals")
def test_signal_killed_child_returncode():
    argv = ["sh", "-c", "kill -TERM $$"]
    result = run(argv)
    assert result.returncode == -signal.SIGTERM
    assert result.returncode == subprocess.run(argv, check=False).returncode
```

(`import signal` at the top of the module; `subprocess` and `IS_WINDOWS` are already there.)

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo test -p toolr-core --lib test_signal_killed_child_returncode_is_negative`, then rebuild the extension
(`uv sync --all-extras --dev` reinstalls the local `toolr-py`) and
`uv run pytest tests/utils/command/test_command.py::test_signal_killed_child_returncode -v`.
Expected: FAIL, `-1 != -15`.

- [ ] **Step 3: Implement**

`let returncode = crate::exit_status::returncode(status);`. In the `run` docstring, say `returncode` is `-N` when
signal `N` killed the child, as with `subprocess`.

- [ ] **Step 4: Run them and confirm they pass**

Run: the Step 2 commands, then `cargo test -p toolr-core --lib command` and `uv run pytest tests/utils/command`.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/command.rs crates/toolr-core/src/command/command_test.rs \
    crates/toolr-py/python/toolr/utils/command.py tests/utils/command/test_command.py
git commit -m "fix(run): return -N from ctx.run for a child killed by signal N"
```

### Task 3: `ctx.exit` and the runner exit `128 + N`

**Files:**

- Modify: `crates/toolr-py/python/toolr/_context.py` (new module-level `status_to_exit_code`; `Context.exit` and its
  docstring)
- Modify: `crates/toolr-py/python/toolr/_runner.py` (`run`, the `except SystemExit` `int` branch; `run` docstring)
- Test: `tests/context/test_exit.py`, `tests/runner/test_runner_coverage.py`, `tests/runner/test_dispatch.py`

**Interfaces:**

- Produces: `toolr._context.status_to_exit_code(status: int) -> int` (private module; not added to `toolr.__all__`).

- [ ] **Step 1: Write the failing tests**

`tests/context/test_exit.py`:

```python
@pytest.mark.parametrize(
    ("status", "expected"),
    [(0, 0), (3, 3), (255, 255), (-15, 143), (-9, 137), (256, 1), (-1000, 1), (-1073741819, 1)],
)
def test_status_to_exit_code(status, expected):
    assert status_to_exit_code(status) == expected


def test_exit_maps_negative_status_to_128_plus_signal(ctx, capfd):
    with pytest.raises(SystemExit) as exc_info:
        ctx.exit(-15, "killed")
    assert exc_info.value.code == 143
    assert "killed" in capfd.readouterr().err
```

(`from toolr._context import status_to_exit_code` at the top. `-1073741819` is `0xC0000005` as an `i32`.)

`tests/runner/test_runner_coverage.py`, next to `test_run_returns_integer_exit_code_from_systemexit` and built the same
way (`fake_target` raising, `_import_target` monkeypatched, `_run_isolated(_runner_spec(repo_root=tmp_path))`):

| Test | `fake_target` raises | Assert |
|---|---|---|
| `test_run_maps_negative_systemexit_to_128_plus_signal` | `SystemExit(-15)` | `== 143` |
| `test_run_maps_out_of_range_systemexit_to_1` | `SystemExit(256)` | `== 1` |

`tests/runner/test_dispatch.py`, next to `test_runner_propagates_nonzero_exit_via_ctx_exit` and built the same way
(`tools_module`, `spec_file`, `run_runner`), skipped on Windows:

```python
@pytest.mark.skipif(sys.platform.startswith("win"), reason="POSIX signals")
def test_runner_exits_128_plus_signal_for_signal_killed_ctx_run(...) -> None:
    # tools module body:
    #     @group.command
    #     def boom(ctx) -> None:
    #         "Do it."
    #         result = ctx.run("sh", "-c", "kill -TERM $$")
    #         ctx.exit(result.returncode, "child was killed")
    assert result.returncode == 143
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `uv run pytest tests/context/test_exit.py tests/runner/test_runner_coverage.py tests/runner/test_dispatch.py -v`
Expected: `ImportError` for `status_to_exit_code`; once that exists as a stub, the mapping tests fail (`-15 != 143`,
the runner returns `-15`, the end-to-end runner exits 241).

- [ ] **Step 3: Implement**

`status_to_exit_code`: `if status < 0: status = 128 - status`; `return status if 0 <= status <= 255 else 1`.
`Context.exit` keeps choosing the message style from the caller's `status`, then calls
`self.parser.exit(status_to_exit_code(status))`. Its docstring says a negative status (a `ctx.run` signal death) exits
`128 + N`, as in a shell. In `_runner.run`, the `isinstance(code, int)` branch returns `status_to_exit_code(code)`
(import it alongside `Context`). Note `bool` is an `int`; `SystemExit(True)` still maps to 1.

- [ ] **Step 4: Run them and confirm they pass**

Run: the Step 2 command, then `uv run pytest tests/context tests/runner`.
Expected: PASS, including `test_runner_propagates_nonzero_exit_via_ctx_exit` (7) and
`test_run_returns_integer_exit_code_from_systemexit` (7).

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-py/python/toolr/_context.py crates/toolr-py/python/toolr/_runner.py \
    tests/context/test_exit.py tests/runner/test_runner_coverage.py tests/runner/test_dispatch.py
git commit -m "fix(context): exit 128 + N for a negative ctx.exit status"
```

### Task 4: Docs, release note, verification, archive

**Files:**

- Modify: `docs/cli.md`, `docs/writing-commands/context.md`, `skills/toolr-command-authoring/SKILL.md`
- Create (via `toolr release-notes add`): `release-notes/<timestamp>.venv-run-signal-exit.md`
- Move: both spec files to `specs/archive/2026/`
- [ ] **Step 1: Write the docs**
    - `docs/cli.md`, `venv run` section, after "pass straight through": "A child killed by a signal exits with 128 plus
      the signal number, as in a shell."
    - `docs/writing-commands/context.md`, after "Inspect `result.returncode` yourself.": "As with `subprocess`, a child
      killed by signal N has `returncode` `-N`. `ctx.exit(result.returncode, ...)` turns that into exit code `128 + N`
      (143 for SIGTERM), as a shell would."
    - `SKILL.md`, the "Calling subprocesses" bullet: add "a signal-killed child has `returncode` `-N`, and
      `ctx.exit(result.returncode)` exits `128 + N`".
- [ ] **Step 2: Queue the release note**

Run: `toolr release-notes add --slug venv-run-signal-exit --text "<the spec's release-note text>"` (the command stages
the file). If the maintainer has ruled the `ctx.run` change breaking, prefix the text with `**Breaking:**` and use
`fix(run)!:` for the Task 2 commit (amend before pushing). Then `toolr release-notes check --base main`.

- [ ] **Step 3: Verify**

Run: `cargo xtask build-skill-refs --check` (regenerate with `cargo xtask build-skill-refs` and commit if the `run` or
`Context.exit` docstring drifted `references/*.md`), then `mise run test` in the background, polling its output every
30–60 s, then `prek run --all-files` and `uv run mkdocs build --strict`. For patch coverage, run the `cargo llvm-cov`
command from `CLAUDE.md` and `cargo xtask patch-coverage --base main`.
Expected: all green, patch coverage at least 99% (the `#[cfg(windows)]` tests run in the Windows CI job only).

- [ ] **Step 4: Commit docs**

```bash
git add docs/cli.md docs/writing-commands/context.md skills/toolr-command-authoring/SKILL.md
git commit -m "docs: document exit codes for signal-killed children"
```

(The release note is already staged by Step 2 and lands in this commit; add any regenerated
`skills/toolr-command-authoring/references/*.md` by path too.)

- [ ] **Step 5: Archive the specs (final commit of the implementing PR)**

```bash
git mv specs/2026-10-31-venv-run-signal-exit-design.md specs/2026-10-31-venv-run-signal-exit-plan.md specs/archive/2026/
git commit -m "docs(specs): archive venv-run-signal-exit spec and plan"
```
