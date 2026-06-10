# Runner path hygiene Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for
> tracking.

**Goal:** Stop the toolr runner from putting the invocation directory on `sys.path`, make `tools.*` resolve
regardless of where toolr was invoked, and run commands from the repo root — without changing the public
`Context` API or the wire format.

**Architecture:** Pass `-P` to the runner interpreter (drops the implicit `''` CWD entry). In the runner,
append `repo_root` to `sys.path` so `tools.*` imports, `os.chdir(repo_root)` before invoking the command, and
emit a double-gated stderr note when relative `Path` args are passed from a subdirectory. The invocation cwd
is a runner-internal local; nothing is added to `Context` or the spec.

**Tech Stack:** Rust (`spawn.rs`), Python (`toolr._runner`), `cargo test`, `pytest`.

**Design:** `specs/2026-06-10-runner-path-hygiene-design.md`. Read it before starting.

**Depends on / stacking:** This branch (`runner-path-hygiene`) is stacked on `static-only-manifest` (SEC-01).
Implement only AFTER SEC-01 has landed on the lower branch and this branch has been restacked onto it — the
plan assumes `_introspect.py` is already deleted and `spawn_runner` is the only `python -m toolr.*` entry
point.

**Conventions (from CLAUDE.md):** Conventional Commits; no `Co-Authored-By` footer; never `--no-verify`; run
`mise run test` for Rust+Python changes; keep markdown prose ≤120 cols (code blocks exempt).

---

## File map

**Modify:**

- `crates/toolr-core/src/execute/spawn.rs` — add `-P` before `-m`.
- `crates/toolr-py/python/toolr/_runner.py` — `run()`: capture invocation cwd, append `repo_root`, warn,
  chdir; add two small helpers.
- `skills/toolr-command-authoring/SKILL.md` (+ any `docs/` authoring page) — document repo-root cwd + path-arg
  semantics.
- `UNRELEASED.md` — release note.

**Create:**

- `tests/runner/test_path_hygiene.py` — Python tests for the helpers and the end-to-end `run()` flow.

---

## Task 1: `-P` flag on the runner spawn

**Files:**

- Modify: `crates/toolr-core/src/execute/spawn.rs:12-21`
- Test: same file (`#[cfg(test)] mod tests`)
- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `spawn.rs`:

```rust
#[test]
fn spawn_runner_passes_safe_path_flag_before_module() {
    // We can't run a real interpreter here; assert the argv we build.
    // Refactor spawn_runner so the args are constructed by a pure helper
    // `runner_args()` returning the Vec<OsString>/Vec<&str> we pass.
    let args = runner_args();
    assert_eq!(args, ["-P", "-m", "toolr._runner"]);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p toolr-core execute::spawn 2>&1 | tail -15`
Expected: FAIL — `runner_args` does not exist yet.

- [ ] **Step 3: Implement**

Extract the arg list into a pure helper and use it in `spawn_runner`:

```rust
/// The fixed argv (after the interpreter) for the runner.
/// `-P` enables safe-path mode (drops the implicit CWD `sys.path` entry);
/// it is a flag, so it is NOT inherited by child processes the command spawns.
fn runner_args() -> [&'static str; 3] {
    ["-P", "-m", "toolr._runner"]
}

pub fn spawn_runner(python: &Path, spec_path: &Path) -> io::Result<Child> {
    Command::new(python)
        .args(runner_args())
        .env("TOOLR_SPEC_FILE", spec_path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p toolr-core execute::spawn 2>&1 | tail -15`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/execute/spawn.rs
git commit -m "feat(execute): pass -P to the runner interpreter (drop implicit CWD from sys.path)"
```

---

## Task 2: `_append_repo_root` helper

**Files:**

- Modify: `crates/toolr-py/python/toolr/_runner.py`
- Test: `tests/runner/test_path_hygiene.py` (create)
- [ ] **Step 1: Write the failing test**

Create `tests/runner/test_path_hygiene.py`:

```python
"""SEC-02: runner sys.path / cwd hygiene."""

from __future__ import annotations

from pathlib import Path

from toolr._runner import _append_repo_root


def test_append_repo_root_adds_when_absent():
    path_list = ["/usr/lib/python3.13", "/site-packages"]
    _append_repo_root("/repo", path_list)
    assert path_list[-1] == "/repo"


def test_append_repo_root_is_idempotent():
    path_list = ["/repo"]
    _append_repo_root("/repo", path_list)
    assert path_list == ["/repo"]
```

- [ ] **Step 2: Run to verify it fails**

Run: `uv run pytest tests/runner/test_path_hygiene.py -q`
Expected: FAIL — `_append_repo_root` does not exist.

- [ ] **Step 3: Implement**

Add to `_runner.py` (module level):

```python
def _append_repo_root(repo_root: str, path_list: list[str] | None = None) -> None:
    """Append ``repo_root`` to ``sys.path`` so ``import tools.*`` resolves.

    Append (not insert) so stdlib and site-packages win — only ``tools.*``,
    which nothing else provides, resolves from the repo. Idempotent.
    """
    import sys  # noqa: PLC0415

    target = sys.path if path_list is None else path_list
    if repo_root not in target:
        target.append(repo_root)
```

- [ ] **Step 4: Run to verify it passes**

Run: `uv run pytest tests/runner/test_path_hygiene.py -q`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-py/python/toolr/_runner.py tests/runner/test_path_hygiene.py
git commit -m "feat(runner): append repo_root to sys.path (resolves tools.* from any cwd)"
```

---

## Task 3: `_warn_if_paths_relative_to_invocation` helper (the double-gated warning)

**Files:**

- Modify: `crates/toolr-py/python/toolr/_runner.py`
- Test: `tests/runner/test_path_hygiene.py`
- [ ] **Step 1: Write the failing tests (the full gating matrix)**

Append to `tests/runner/test_path_hygiene.py`:

```python
import io

from toolr._runner import _warn_if_paths_relative_to_invocation as _warn


def _run(invocation_cwd, repo_root, values):
    stream = io.StringIO()
    _warn(Path(invocation_cwd), Path(repo_root), values, stream)
    return stream.getvalue()


def test_warns_on_relative_path_arg_from_subdir():
    out = _run("/repo/sub", "/repo", [Path("x.py")])
    assert "repo root" in out and "/repo" in out


def test_no_warn_when_cwd_is_repo_root():
    assert _run("/repo", "/repo", [Path("x.py")]) == ""


def test_no_warn_without_path_args():
    assert _run("/repo/sub", "/repo", ["x.py", 3, True]) == ""


def test_no_warn_for_absolute_path_arg():
    assert _run("/repo/sub", "/repo", [Path("/abs/x.py")]) == ""


def test_warns_for_relative_path_inside_list():
    out = _run("/repo/sub", "/repo", [[Path("a.py"), Path("/abs/b.py")]])
    assert "repo root" in out
```

- [ ] **Step 2: Run to verify it fails**

Run: `uv run pytest tests/runner/test_path_hygiene.py -q`
Expected: FAIL — `_warn_if_paths_relative_to_invocation` does not exist.

- [ ] **Step 3: Implement**

Add to `_runner.py`:

```python
def _warn_if_paths_relative_to_invocation(
    invocation_cwd: Path,
    repo_root: Path,
    values,
    stream,
) -> None:
    """Emit one note iff cwd != repo_root AND a coerced arg is a relative Path.

    Type-driven, never heuristic: only ``pathlib.Path`` instances (which all
    ``toolr.types`` path-constrained args coerce to) count. ``str`` args are
    never inspected for path-likeness.
    """
    if invocation_cwd.resolve() == repo_root.resolve():
        return

    def _is_rel_path(v) -> bool:
        if isinstance(v, Path):
            return not v.is_absolute()
        if isinstance(v, (list, tuple)):
            return any(isinstance(x, Path) and not x.is_absolute() for x in v)
        return False

    if any(_is_rel_path(v) for v in values):
        print(  # noqa: T201
            f"toolr: note: commands run from the repo root ({repo_root}); "
            f"relative path arguments resolve from there, not {invocation_cwd}",
            file=stream,
        )
```

Add `from pathlib import Path` to the module imports if not already top-level (check the file; the runner
uses lazy `import pathlib` in places — a module-level `from pathlib import Path` for these helpers is fine).

- [ ] **Step 4: Run to verify it passes**

Run: `uv run pytest tests/runner/test_path_hygiene.py -q`
Expected: PASS (all five cases).

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-py/python/toolr/_runner.py tests/runner/test_path_hygiene.py
git commit -m "feat(runner): double-gated relative-path warning helper"
```

---

## Task 4: Wire capture-cwd, append, warn, chdir into `run()`

**Files:**

- Modify: `crates/toolr-py/python/toolr/_runner.py:413-480` (`run()`)
- Test: `tests/runner/test_path_hygiene.py`
- [ ] **Step 1: Write the failing end-to-end test**

This exercises the real `run()` flow with a temp `tools` package, asserting (a) cwd becomes repo_root and
(b) `tools.*` imports when invoked from a subdirectory. Save/restore cwd and `sys.path` so the test is
hermetic.

```python
import os
import sys
import textwrap

import msgspec

from toolr._runner import RunnerSpec, run


def _make_repo(tmp_path):
    repo = tmp_path / "repo"
    (repo / "tools").mkdir(parents=True)
    (repo / "tools" / "__init__.py").write_text("")
    (repo / "tools" / "probe.py").write_text(
        textwrap.dedent(
            """
            import os
            CWD_AT_CALL = {}
            def record(ctx):
                CWD_AT_CALL["cwd"] = os.getcwd()
            """
        )
    )
    return repo


def _spec(repo, module, function):
    # Build a minimal valid spec dict; adapt field names to RunnerSpec.
    # Use the existing spec-construction test helpers in tests/runner/ as a
    # template (see tests/runner/test_spec_loader.py / test_dispatch.py).
    payload = {
        "schema_version": __import__("toolr._runner", fromlist=["SCHEMA_VERSION"]).SCHEMA_VERSION,
        "group": "probe",
        "command": "record",
        "module": module,
        "function": function,
        "args": [],
        "dispatch": None,
        "context": {
            "repo_root": str(repo),
            "verbosity": "normal",
            "default_timeout_secs": None,
            "default_no_output_timeout_secs": None,
        },
    }
    return msgspec.convert(payload, type=RunnerSpec)


def test_run_chdirs_to_repo_root_and_imports_tools_from_subdir(tmp_path, monkeypatch):
    repo = _make_repo(tmp_path)
    sub = repo / "tools"  # a subdirectory of the repo
    saved_path = sys.path[:]
    saved_cwd = os.getcwd()
    try:
        monkeypatch.chdir(sub)  # invoke from a subdirectory
        spec = _spec(repo, "tools.probe", "record")
        rc = run(spec)
        assert rc == 0
        import tools.probe  # resolvable because repo_root was appended

        assert tools.probe.CWD_AT_CALL["cwd"] == str(repo)
    finally:
        sys.path[:] = saved_path
        os.chdir(saved_cwd)
        sys.modules.pop("tools.probe", None)
        sys.modules.pop("tools", None)
```

NOTE: confirm the exact `RunnerSpec`/`context` field names and the spec-construction pattern against
`tests/runner/test_spec_loader.py` and `crates/toolr-py/python/toolr/_runner.py` before finalizing this test
— adapt the payload dict to match. Keep imports top-of-file (CLAUDE.md: no deferred imports in tests).

- [ ] **Step 2: Run to verify it fails**

Run: `uv run pytest tests/runner/test_path_hygiene.py -q`
Expected: FAIL — `run()` does not yet append repo_root or chdir (the `import tools.probe` or the cwd
assertion fails).

- [ ] **Step 3: Implement the `run()` changes**

At the top of `run()` (before the `try`/before `_build_context`), capture the invocation cwd and repo_root;
append repo_root before `_import_target`; warn + chdir before invoking the target. Apply to BOTH the dispatch
and non-dispatch branches.

```python
def run(spec: RunnerSpec) -> int:  # noqa: PLR0911
    import os  # noqa: PLC0415
    import sys  # noqa: PLC0415

    invocation_cwd = Path.cwd()
    repo_root = Path(spec.context.repo_root)
    try:
        ctx = _build_context(spec)
        _append_repo_root(str(repo_root))
        target = _import_target(spec)
        if spec.dispatch is not None:
            _, parent_kwargs = _coerce_args(target, spec.args)
            _warn_if_paths_relative_to_invocation(
                invocation_cwd, repo_root, list(parent_kwargs.values()), sys.stderr
            )
            _chdir_or_raise(repo_root)
            invoke_dispatcher(
                ctx=ctx,
                func=target,
                parent_kwargs=parent_kwargs,
                child_name=spec.dispatch.command,
                child_args=spec.dispatch.command_args,
                child_schema=spec.dispatch.schema,
            )
        else:
            var_args, kw_args = _coerce_args(target, spec.args)
            _warn_if_paths_relative_to_invocation(
                invocation_cwd, repo_root, [*var_args, *kw_args.values()], sys.stderr
            )
            _chdir_or_raise(repo_root)
            target(ctx, *var_args, **kw_args)
    except SystemExit as exc:
        ...  # unchanged
```

Add the small chdir helper (clear error instead of a raw OSError):

```python
def _chdir_or_raise(repo_root: Path) -> None:
    import os  # noqa: PLC0415

    try:
        os.chdir(repo_root)
    except OSError as exc:
        msg = f"failed to enter repo root {repo_root}: {exc}"
        raise SpecError(msg) from exc
```

KNOWN LIMITATION (document in the design's §5 if not already): in the dispatch path, only the parent's
coerced kwargs are checked for relative paths; a dispatched child's relative path args are coerced inside
`invoke_dispatcher` and are not covered by this warning. Acceptable — the warning is best-effort.

- [ ] **Step 4: Run to verify it passes**

Run: `uv run pytest tests/runner/test_path_hygiene.py -q`
Expected: PASS.

- [ ] **Step 5: Run the full runner suite (no regressions)**

Run: `uv run pytest tests/runner -q`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/toolr-py/python/toolr/_runner.py tests/runner/test_path_hygiene.py
git commit -m "feat(runner): run commands from repo root; warn on relative path args from a subdir (SEC-02)"
```

---

## Task 5: End-to-end `-P` shadowing proof (best-effort, real interpreter)

The `-P` flag only takes effect at interpreter startup, so the "planted module is not imported" proof needs a
real runner spawn (not an in-process `run()` call).

- [ ] **Step 1: Find the existing venv-backed dispatch harness**

Run: `grep -rln 'venv\|build.*wheel\|toolr-py' tests/distribution tests/sources/test_e2e.py`
Determine whether there is an existing harness that builds a tools venv with `toolr-py` installed and
dispatches a real command (likely `tests/distribution/` or `tests/sources/test_e2e.py`).

- [ ] **Step 2: Add the shadowing test where that harness lives**

If a venv-backed harness exists, add a test: create a repo whose **invocation directory** contains a
`msgspec.py` that raises on import; dispatch a real command; assert it succeeds (proving `''` is not on
`sys.path`, so the planted `msgspec.py` was never imported). Mark it with the suite's slow/distribution
marker if appropriate.

```python
# pseudo-shape — adapt to the real harness fixtures:
def test_safe_path_ignores_planted_module(real_tools_venv_repo):
    repo = real_tools_venv_repo  # has tools/ + a synced venv with toolr-py
    (repo / "msgspec.py").write_text("raise RuntimeError('planted module imported')")
    result = run_toolr(["probe", "record"], cwd=repo)  # invoke from repo root
    assert result.returncode == 0  # would be != 0 if msgspec.py shadowed the real one
```

- [ ] **Step 3: If NO such harness exists, do not fake it**

Per CLAUDE.md "no silent caps": if building a real venv in-test is impractical here, SKIP this end-to-end
test, and instead add a one-line comment in `tests/runner/test_path_hygiene.py` recording that the `-P`
runtime effect is covered only by the `spawn_runner` argv unit test (Task 1) plus manual verification, not an
automated shadowing test. Report this gap in your final summary.

- [ ] **Step 4: Commit (if a test was added)**

```bash
git add tests/
git commit -m "test(runner): -P drops planted CWD module from sys.path (end-to-end)"
```

---

## Task 6: Document the contract

**Files:**

- Modify: `skills/toolr-command-authoring/SKILL.md` (+ any `docs/` authoring page)

- [ ] **Step 1: Add the section**

State: *Commands run with the working directory set to the repo root, regardless of where you invoke
`toolr`. Relative path arguments are therefore resolved from the repo root, not your current directory. Use
absolute paths if you need to refer to files relative to where you ran the command.*

- [ ] **Step 2: Regenerate skill refs (if the public surface changed)**

Run: `cargo xtask build-skill-refs` then commit any regenerated `references/*.md`.

- [ ] **Step 3: Commit**

```bash
git add skills/ docs/
git commit -m "docs(authoring): commands run from the repo root; path args are repo-root-relative"
```

---

## Task 7: Release note

**Files:**

- Modify: `UNRELEASED.md`

- [ ] **Step 1: Add entries** (never edit `CHANGELOG.md`)

```markdown
### Security

- The toolr runner no longer puts the invocation directory on `sys.path` (the interpreter is started with
  `-P`), preventing a stray `.py` file in your current directory from shadowing stdlib/site-packages modules.

### Changed

- Commands now run with the working directory set to the repo root (like `make`/`cargo`). Relative path
  arguments resolve from the repo root, not your current directory; toolr prints a one-line note if you pass
  a relative path from a subdirectory.
```

- [ ] **Step 2: Commit**

```bash
git add UNRELEASED.md
git commit -m "docs(unreleased): note runner -P + repo-root cwd (SEC-02)"
```

---

## Task 8: Full verification & audit status

- [ ] **Step 1: Umbrella suite**

Run: `mise run test` (poll long runs every 30–60s).
Expected: skill-ref gate, `cargo test --workspace`, and `pytest` all green.

- [ ] **Step 2: Targeted**

Run: `cargo test -p toolr-core execute::spawn` and `uv run pytest tests/runner -q`
Expected: PASS.

- [ ] **Step 3: Audit status (do in the MAIN working tree, not this worktree — `audit/` is untracked there)**

Flip SEC-02 to `Done: runner-path-hygiene` in `audit/2026-06-10/README.md` and the SEC-02 finding file. If
you are a worktree sub-agent, skip this and report instead — the orchestrator will update the audit.

> Done when: `mise run test` is green; `spawn_runner` passes `-P`; the runner appends repo_root, chdirs to it,
> and the warning fires only under both gates; docs updated.

---

## Self-review notes (author)

- **Design coverage:** §1 `-P` (Task 1), §2 append (Task 2), §3 chdir (Task 4), §4 documented (Task 6), §5
  warning (Task 3 + wired in Task 4). ✓
- **No public-API/wire change:** invocation cwd is a local in `run()`; `Context` and the spec are untouched;
  no schema bump. ✓
- **Known limitation captured:** dispatch-child relative-path args aren't covered by the warning (Task 4
  Step 3). ✓
- **Test-reality check:** the in-process `run()` test (Task 4) covers append + chdir deterministically; the
  `-P` runtime effect needs a real spawn (Task 5) — flagged with a no-silent-cap fallback if no venv harness
  exists. ✓
- **Implementer must confirm:** exact `RunnerSpec`/`context` field names for the Task 4 spec payload (against
  `tests/runner/test_spec_loader.py`) before finalizing.

```text
