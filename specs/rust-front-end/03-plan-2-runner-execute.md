<!-- rumdl-disable MD046 MD076 -->

# Plan 2: Python Runner + Execute Model (S1)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Lint:** Plan docs nest fenced code inside list items for step-by-step
> structure. The `<!-- rumdl-disable MD046 MD076 -->` directive above turns
> off the code-block-style and list-item-spacing rules for this file only.

**Goal:** Replace the exit-64 stub left by Plan 1's Task 15 with a real
execution path. Invoking `toolr <group> <command> [<args>...]` now writes a
spec JSON to a tempfile, spawns `<python> -m toolr._runner` with
`TOOLR_SPEC_FILE` pointing at it, forwards stdio and signals, and propagates
the child's exit code. End state: `toolr ci hello` (where `tools/ci.py`
defines a `hello` command) produces identical user-visible behavior to
today's argparse path, all driven by the Rust front-end.

**Architecture:** A new `toolr._runner` Python module ships inside the
existing `toolr` Python package. It reads `$TOOLR_SPEC_FILE`, decodes it with
`msgspec.json.decode(..., type=SpecSchema)`, imports the declared
`tools.<module>`, builds a `Context`, dispatches argv-style args (already
parsed by clap on the Rust side) to the target function. On the Rust side, a
new `execute` module under `_rust_utils` builds the `ExecutionSpec`, writes
it to a `tempfile::NamedTempFile`, spawns the child with inherited stdio,
installs an OS-specific signal handler that forwards SIGINT/SIGTERM, waits
for exit, and returns the child's exit code to `dispatch::dispatch`. Python
discovery for v1 is intentionally minimal — Plan 3 owns the tools venv; here
we read `TOOLR_PYTHON` (env override) or fall back to `python3` on PATH.

**Tech Stack:** Rust 2021, `serde`/`serde_json`, `tempfile`, `anyhow`,
`thiserror`, `assert_cmd` (integration tests). Python 3.11+, `msgspec`
(already a `toolr` package dep), stdlib only otherwise.

**Reading order in this plan:** Tasks build on each other. Tasks 1–3 land
the Python runner shim and its tests in isolation. Tasks 4–8 build the Rust
side. Task 9 wires both halves through `dispatch.rs` and adds the end-to-end
smoke. Task 10 handles signals. Task 11 documents the runner module in
`__init__.py`/`__all__`. Task 12 updates the roadmap.

---

## Task 1: Define the runner spec schema in Python

Create a `msgspec.Struct`-based schema that matches the JSON payload the Rust
side will write. This is the single source of truth on the Python side.

**Files:**

- Create: `python/toolr/_runner.py`
- Create: `tests/runner/__init__.py`
- Create: `tests/runner/test_spec_schema.py`

- [ ] **Step 1.1: Create the empty test package marker**

    Create `tests/runner/__init__.py`:

    ```python
    ```

    (Empty file — just marks `tests/runner/` as a package for pytest collection.)

- [ ] **Step 1.2: Write failing schema tests in `tests/runner/test_spec_schema.py`**

    ```python
    from __future__ import annotations

    import msgspec
    import pytest

    from toolr._runner import ContextSpec
    from toolr._runner import RunnerSpec
    from toolr._runner import SCHEMA_VERSION


    def test_schema_version_constant_is_1() -> None:
        assert SCHEMA_VERSION == 1


    def test_runner_spec_round_trips_through_json() -> None:
        spec = RunnerSpec(
            schema_version=SCHEMA_VERSION,
            group="ci",
            command="hello",
            module="tools.ci",
            function="hello",
            args={"name": "Alice"},
            context=ContextSpec(
                repo_root="/tmp/repo",
                verbosity="normal",
                timestamps=False,
                log_level="INFO",
            ),
        )
        encoded = msgspec.json.encode(spec)
        decoded = msgspec.json.decode(encoded, type=RunnerSpec)
        assert decoded == spec


    def test_runner_spec_rejects_unknown_schema_version() -> None:
        payload = {
            "schema_version": 999,
            "group": "ci",
            "command": "hello",
            "module": "tools.ci",
            "function": "hello",
            "args": {},
            "context": {
                "repo_root": "/tmp/repo",
                "verbosity": "normal",
                "timestamps": False,
                "log_level": "INFO",
            },
        }
        # We decode successfully (msgspec doesn't reject the int itself), but the
        # runner's higher-level check (Task 3) raises on version mismatch.
        decoded = msgspec.json.decode(msgspec.json.encode(payload), type=RunnerSpec)
        assert decoded.schema_version == 999


    def test_runner_spec_rejects_missing_required_field() -> None:
        payload = {
            "schema_version": 1,
            "group": "ci",
            # missing "command"
            "module": "tools.ci",
            "function": "hello",
            "args": {},
            "context": {
                "repo_root": "/tmp/repo",
                "verbosity": "normal",
                "timestamps": False,
                "log_level": "INFO",
            },
        }
        with pytest.raises(msgspec.ValidationError):
            msgspec.json.decode(msgspec.json.encode(payload), type=RunnerSpec)
    ```

- [ ] **Step 1.3: Run tests and verify they FAIL**

    ```bash
    uv run pytest tests/runner/test_spec_schema.py -x
    ```

    Expected: `ImportError: cannot import name 'ContextSpec' from 'toolr._runner'` (the module does not exist yet).

- [ ] **Step 1.4: Create the minimal `python/toolr/_runner.py` with just the schema**

    ```python
    """Runner shim: invoked as ``python -m toolr._runner`` by the toolr binary.

    Reads the spec JSON path from ``$TOOLR_SPEC_FILE``, decodes it with
    ``msgspec.json``, imports the target module, builds a ``Context``, and
    calls the target function. Exit code propagates to the parent toolr
    process and on to the shell.
    """

    from __future__ import annotations

    from typing import Any

    import msgspec

    SCHEMA_VERSION: int = 1


    class ContextSpec(msgspec.Struct, frozen=True):
        """Subset of the ``Context`` reconstructable from the Rust front-end."""

        repo_root: str
        verbosity: str
        timestamps: bool
        log_level: str


    class RunnerSpec(msgspec.Struct, frozen=True):
        """Top-level spec written by the Rust binary into ``$TOOLR_SPEC_FILE``."""

        schema_version: int
        group: str
        command: str
        module: str
        function: str
        args: dict[str, Any]
        context: ContextSpec
    ```

- [ ] **Step 1.5: Run tests and verify they PASS**

    ```bash
    uv run pytest tests/runner/test_spec_schema.py -v
    ```

    Expected: 4 tests passing.

- [ ] **Step 1.6: Commit**

    ```bash
    git add python/toolr/_runner.py tests/runner/__init__.py tests/runner/test_spec_schema.py
    git commit -m "feat(runner): Add msgspec schema for toolr._runner spec payload"
    ```

---

## Task 2: Loader and version validation in the runner

Wire reading-from-disk + schema-version validation. Still no module imports
yet — the goal is to verify we can pull a spec file off disk, reject bad
versions, and surface clear errors.

**Files:**

- Modify: `python/toolr/_runner.py`
- Create: `tests/runner/test_spec_loader.py`

- [ ] **Step 2.1: Write failing loader tests in `tests/runner/test_spec_loader.py`**

    ```python
    from __future__ import annotations

    import json
    import os
    from pathlib import Path

    import pytest

    from toolr._runner import SCHEMA_VERSION
    from toolr._runner import SpecError
    from toolr._runner import load_spec


    def _write_spec(tmp_path: Path, **overrides: object) -> Path:
        payload: dict[str, object] = {
            "schema_version": SCHEMA_VERSION,
            "group": "ci",
            "command": "hello",
            "module": "tools.ci",
            "function": "hello",
            "args": {},
            "context": {
                "repo_root": str(tmp_path),
                "verbosity": "normal",
                "timestamps": False,
                "log_level": "INFO",
            },
        }
        payload.update(overrides)
        spec_path = tmp_path / "spec.json"
        spec_path.write_text(json.dumps(payload))
        return spec_path


    def test_load_spec_reads_file_and_decodes(tmp_path: Path) -> None:
        spec_path = _write_spec(tmp_path)
        spec = load_spec(spec_path)
        assert spec.group == "ci"
        assert spec.command == "hello"
        assert spec.context.repo_root == str(tmp_path)


    def test_load_spec_rejects_unknown_schema_version(tmp_path: Path) -> None:
        spec_path = _write_spec(tmp_path, schema_version=999)
        with pytest.raises(SpecError) as exc_info:
            load_spec(spec_path)
        assert "schema_version" in str(exc_info.value)
        assert "999" in str(exc_info.value)


    def test_load_spec_raises_when_file_missing(tmp_path: Path) -> None:
        with pytest.raises(SpecError) as exc_info:
            load_spec(tmp_path / "absent.json")
        assert "not found" in str(exc_info.value).lower() or "no such" in str(exc_info.value).lower()


    def test_load_spec_raises_on_malformed_json(tmp_path: Path) -> None:
        spec_path = tmp_path / "bad.json"
        spec_path.write_text("{not json")
        with pytest.raises(SpecError):
            load_spec(spec_path)


    def test_load_spec_from_env(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        from toolr._runner import load_spec_from_env

        spec_path = _write_spec(tmp_path)
        monkeypatch.setenv("TOOLR_SPEC_FILE", str(spec_path))
        spec = load_spec_from_env()
        assert spec.group == "ci"


    def test_load_spec_from_env_raises_when_unset(monkeypatch: pytest.MonkeyPatch) -> None:
        from toolr._runner import load_spec_from_env

        monkeypatch.delenv("TOOLR_SPEC_FILE", raising=False)
        with pytest.raises(SpecError) as exc_info:
            load_spec_from_env()
        assert "TOOLR_SPEC_FILE" in str(exc_info.value)
    ```

- [ ] **Step 2.2: Run tests and verify they FAIL**

    ```bash
    uv run pytest tests/runner/test_spec_loader.py -x
    ```

    Expected: `ImportError: cannot import name 'SpecError'` (or `load_spec`).

- [ ] **Step 2.3: Extend `python/toolr/_runner.py` with the loader**

    Replace the file with:

    ```python
    """Runner shim: invoked as ``python -m toolr._runner`` by the toolr binary.

    Reads the spec JSON path from ``$TOOLR_SPEC_FILE``, decodes it with
    ``msgspec.json``, imports the target module, builds a ``Context``, and
    calls the target function. Exit code propagates to the parent toolr
    process and on to the shell.
    """

    from __future__ import annotations

    import os
    from pathlib import Path
    from typing import Any

    import msgspec

    SCHEMA_VERSION: int = 1

    _SPEC_ENV_VAR = "TOOLR_SPEC_FILE"


    class SpecError(Exception):
        """Raised when the spec file is missing, malformed, or unsupported."""


    class ContextSpec(msgspec.Struct, frozen=True):
        """Subset of the ``Context`` reconstructable from the Rust front-end."""

        repo_root: str
        verbosity: str
        timestamps: bool
        log_level: str


    class RunnerSpec(msgspec.Struct, frozen=True):
        """Top-level spec written by the Rust binary into ``$TOOLR_SPEC_FILE``."""

        schema_version: int
        group: str
        command: str
        module: str
        function: str
        args: dict[str, Any]
        context: ContextSpec


    def load_spec(path: str | os.PathLike[str]) -> RunnerSpec:
        """Read the spec at ``path`` and decode it into a :class:`RunnerSpec`.

        Validates the schema version and raises :class:`SpecError` on any
        problem (missing file, malformed JSON, unsupported schema version).
        """
        spec_path = Path(path)
        try:
            data = spec_path.read_bytes()
        except FileNotFoundError as exc:
            msg = f"toolr spec file not found: {spec_path}"
            raise SpecError(msg) from exc
        except OSError as exc:
            msg = f"failed to read toolr spec file {spec_path}: {exc}"
            raise SpecError(msg) from exc
        try:
            spec = msgspec.json.decode(data, type=RunnerSpec)
        except msgspec.DecodeError as exc:
            msg = f"toolr spec file is not valid JSON ({spec_path}): {exc}"
            raise SpecError(msg) from exc
        except msgspec.ValidationError as exc:
            msg = f"toolr spec file failed schema validation ({spec_path}): {exc}"
            raise SpecError(msg) from exc
        if spec.schema_version != SCHEMA_VERSION:
            msg = (
                f"toolr spec file declares schema_version={spec.schema_version}, "
                f"but this toolr Python package only supports {SCHEMA_VERSION}. "
                "Upgrade the toolr package in your tools venv."
            )
            raise SpecError(msg)
        return spec


    def load_spec_from_env() -> RunnerSpec:
        """Read ``$TOOLR_SPEC_FILE`` and call :func:`load_spec` on it."""
        spec_path = os.environ.get(_SPEC_ENV_VAR)
        if not spec_path:
            msg = (
                f"{_SPEC_ENV_VAR} is not set. The toolr runner must be invoked "
                "by the toolr binary, not directly."
            )
            raise SpecError(msg)
        return load_spec(spec_path)
    ```

- [ ] **Step 2.4: Run tests, expect PASS**

    ```bash
    uv run pytest tests/runner/test_spec_loader.py -v
    ```

    Expected: 6 tests passing.

- [ ] **Step 2.5: Commit**

    ```bash
    git add python/toolr/_runner.py tests/runner/test_spec_loader.py
    git commit -m "feat(runner): Load + validate spec JSON from TOOLR_SPEC_FILE"
    ```

---

## Task 3: Dispatch into the target function

Import the user's module, build a `Context`, and call the target function
with the parsed args. Plus a `main()` entry point so `python -m
toolr._runner` works.

**Files:**

- Modify: `python/toolr/_runner.py`
- Create: `tests/runner/test_dispatch.py`

- [ ] **Step 3.1: Write failing dispatch tests in `tests/runner/test_dispatch.py`**

    ```python
    from __future__ import annotations

    import json
    import os
    import subprocess
    import sys
    import textwrap
    from pathlib import Path

    import pytest

    from toolr._runner import SCHEMA_VERSION


    def _write_tools_module(tools_dir: Path, body: str) -> None:
        tools_dir.mkdir(parents=True, exist_ok=True)
        (tools_dir / "__init__.py").write_text("")
        (tools_dir / "demo.py").write_text(textwrap.dedent(body))


    def _write_spec(spec_path: Path, repo_root: Path, *, command: str, function: str, args: dict[str, object]) -> None:
        payload = {
            "schema_version": SCHEMA_VERSION,
            "group": "demo",
            "command": command,
            "module": "tools.demo",
            "function": function,
            "args": args,
            "context": {
                "repo_root": str(repo_root),
                "verbosity": "normal",
                "timestamps": False,
                "log_level": "INFO",
            },
        }
        spec_path.write_text(json.dumps(payload))


    def _run_runner(spec_path: Path, repo_root: Path) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        env["TOOLR_SPEC_FILE"] = str(spec_path)
        env["PYTHONPATH"] = str(repo_root) + os.pathsep + env.get("PYTHONPATH", "")
        return subprocess.run(
            [sys.executable, "-m", "toolr._runner"],
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )


    def test_runner_invokes_target_function(tmp_path: Path) -> None:
        _write_tools_module(
            tmp_path / "tools",
            """
            from toolr import command_group

            group = command_group("demo", "Demo", description="demo group")

            @group.command
            def hello(ctx, name: str = "world") -> None:
                ctx.print(f"hi {name}")
            """,
        )
        spec_path = tmp_path / "spec.json"
        _write_spec(spec_path, tmp_path, command="hello", function="hello", args={"name": "Alice"})

        result = _run_runner(spec_path, tmp_path)
        assert result.returncode == 0, f"stderr:\n{result.stderr}\nstdout:\n{result.stdout}"
        assert "hi Alice" in result.stdout


    def test_runner_propagates_nonzero_exit_via_ctx_exit(tmp_path: Path) -> None:
        _write_tools_module(
            tmp_path / "tools",
            """
            from toolr import command_group

            group = command_group("demo", "Demo", description="demo group")

            @group.command
            def boom(ctx) -> None:
                ctx.exit(7, "failing on purpose")
            """,
        )
        spec_path = tmp_path / "spec.json"
        _write_spec(spec_path, tmp_path, command="boom", function="boom", args={})

        result = _run_runner(spec_path, tmp_path)
        assert result.returncode == 7


    def test_runner_propagates_exception_as_exit_1(tmp_path: Path) -> None:
        _write_tools_module(
            tmp_path / "tools",
            """
            from toolr import command_group

            group = command_group("demo", "Demo", description="demo group")

            @group.command
            def crash(ctx) -> None:
                raise RuntimeError("crashed")
            """,
        )
        spec_path = tmp_path / "spec.json"
        _write_spec(spec_path, tmp_path, command="crash", function="crash", args={})

        result = _run_runner(spec_path, tmp_path)
        assert result.returncode == 1
        assert "RuntimeError" in result.stderr
        assert "crashed" in result.stderr


    def test_runner_fails_clearly_when_spec_env_unset(tmp_path: Path) -> None:
        env = os.environ.copy()
        env.pop("TOOLR_SPEC_FILE", None)
        result = subprocess.run(
            [sys.executable, "-m", "toolr._runner"],
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )
        assert result.returncode != 0
        assert "TOOLR_SPEC_FILE" in result.stderr
    ```

- [ ] **Step 3.2: Run tests and verify they FAIL**

    ```bash
    uv run pytest tests/runner/test_dispatch.py -x
    ```

    Expected: failure — `python -m toolr._runner` has no entry point yet (or fails because dispatch isn't implemented).

- [ ] **Step 3.3: Append the dispatch + `main()` to `python/toolr/_runner.py`**

    Append the following to the end of `python/toolr/_runner.py`:

    ```python


    def _build_context(spec: RunnerSpec) -> "Context":  # noqa: F821
        """Construct a minimal :class:`toolr.Context` from a :class:`RunnerSpec`."""
        # Late imports — keep module-load fast and avoid pulling rich into pure
        # spec-decoding code paths.
        import pathlib  # noqa: PLC0415
        from argparse import ArgumentParser  # noqa: PLC0415

        from toolr._context import Context  # noqa: PLC0415
        from toolr.utils._console import ConsoleVerbosity  # noqa: PLC0415
        from toolr.utils._console import Consoles  # noqa: PLC0415

        verbosity_map = {
            "quiet": ConsoleVerbosity.QUIET,
            "normal": ConsoleVerbosity.NORMAL,
            "verbose": ConsoleVerbosity.VERBOSE,
        }
        try:
            verbosity = verbosity_map[spec.context.verbosity]
        except KeyError as exc:
            msg = (
                f"unknown verbosity {spec.context.verbosity!r} in spec; "
                f"expected one of {sorted(verbosity_map)}"
            )
            raise SpecError(msg) from exc

        consoles = Consoles.setup(verbosity)
        # ArgumentParser is required by Context for ctx.exit() — it calls
        # parser.exit(status). A bare parser is sufficient.
        parser = ArgumentParser(prog=f"toolr {spec.group} {spec.command}", add_help=False)
        return Context(
            repo_root=pathlib.Path(spec.context.repo_root),
            parser=parser,
            verbosity=verbosity,
            _console_stderr=consoles.stderr,
            _console_stdout=consoles.stdout,
        )


    def _import_target(spec: RunnerSpec) -> Any:
        """Import ``spec.module`` and return the attribute named ``spec.function``."""
        import importlib  # noqa: PLC0415

        try:
            module = importlib.import_module(spec.module)
        except ImportError as exc:
            msg = f"failed to import {spec.module}: {exc}"
            raise SpecError(msg) from exc
        try:
            return getattr(module, spec.function)
        except AttributeError as exc:
            msg = f"module {spec.module!r} has no attribute {spec.function!r}"
            raise SpecError(msg) from exc


    def run(spec: RunnerSpec) -> int:
        """Execute the command described by ``spec``. Returns a process exit code.

        ``ctx.exit(status, ...)`` raises :class:`SystemExit`; we honor its code.
        Any other uncaught exception is logged to stderr and returns 1.
        """
        try:
            ctx = _build_context(spec)
            target = _import_target(spec)
            target(ctx, **spec.args)
        except SystemExit as exc:
            code = exc.code
            if code is None:
                return 0
            if isinstance(code, int):
                return code
            # str / other: print and return 1
            import sys  # noqa: PLC0415

            print(code, file=sys.stderr)
            return 1
        except SpecError as exc:
            import sys  # noqa: PLC0415

            print(f"toolr runner: {exc}", file=sys.stderr)
            return 2
        except BaseException:
            import sys  # noqa: PLC0415
            import traceback  # noqa: PLC0415

            traceback.print_exc(file=sys.stderr)
            return 1
        return 0


    def main() -> int:
        """Module entry point — invoked by ``python -m toolr._runner``."""
        try:
            spec = load_spec_from_env()
        except SpecError as exc:
            import sys  # noqa: PLC0415

            print(f"toolr runner: {exc}", file=sys.stderr)
            return 2
        return run(spec)


    if __name__ == "__main__":
        import sys  # noqa: PLC0415

        sys.exit(main())
    ```

- [ ] **Step 3.4: Run tests, expect PASS**

    ```bash
    uv run pytest tests/runner/test_dispatch.py -v
    ```

    Expected: 4 tests passing.

- [ ] **Step 3.5: Manual smoke (optional but recommended)**

    ```bash
    uv run python -c 'import toolr._runner as r; print(r.SCHEMA_VERSION, r.RunnerSpec)'
    ```

    Expected: `1 <class 'toolr._runner.RunnerSpec'>`.

- [ ] **Step 3.6: Commit**

    ```bash
    git add python/toolr/_runner.py tests/runner/test_dispatch.py
    git commit -m "feat(runner): Dispatch into target function via Context"
    ```

---

## Task 4: Rust-side spec types

Add `_rust_utils::execute::spec` with `ExecutionSpec` and `ContextSpec`
serde-derived structs that match the Python schema field-by-field. Round-trip
tests prove wire compatibility.

**Files:**

- Create: `src/execute/mod.rs`
- Create: `src/execute/spec.rs`
- Modify: `src/lib.rs`

- [ ] **Step 4.1: Expose the `execute` module from `src/lib.rs`**

    Add to `src/lib.rs`, alongside the other `pub mod` lines:

    ```rust
    pub mod execute;
    ```

- [ ] **Step 4.2: Create `src/execute/mod.rs`**

    ```rust
    //! Subprocess execution of user commands via `python -m toolr._runner`.

    pub mod spec;

    pub use spec::{ContextSpec, ExecutionSpec, RUNNER_SCHEMA_VERSION};
    ```

- [ ] **Step 4.3: Create `src/execute/spec.rs`**

    ```rust
    //! Serde-derived types matching the Python runner's spec schema.
    //!
    //! Wire format: JSON. The Python side decodes with
    //! `msgspec.json.decode(data, type=RunnerSpec)`. Field names and types
    //! here must stay in lock-step with `python/toolr/_runner.py`.

    use std::collections::BTreeMap;

    use serde::{Deserialize, Serialize};

    /// Schema version. Must match `toolr._runner.SCHEMA_VERSION` exactly.
    pub const RUNNER_SCHEMA_VERSION: u32 = 1;

    /// Reduced view of `toolr.Context` reconstructable from JSON.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct ContextSpec {
        pub repo_root: String,
        /// One of "quiet", "normal", "verbose".
        pub verbosity: String,
        pub timestamps: bool,
        /// Python `logging` level name, e.g. "INFO".
        pub log_level: String,
    }

    /// Top-level execution spec written to `$TOOLR_SPEC_FILE`.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct ExecutionSpec {
        pub schema_version: u32,
        pub group: String,
        pub command: String,
        pub module: String,
        pub function: String,
        /// Argument map: name → JSON value (string / number / bool / null).
        /// We use `serde_json::Value` (via `BTreeMap` for deterministic
        /// ordering in tests) so callers can pass parsed clap matches through
        /// without per-arg type juggling on the Rust side.
        pub args: BTreeMap<String, serde_json::Value>,
        pub context: ContextSpec,
    }

    impl ExecutionSpec {
        /// Construct a default-shaped spec with empty args and a quiet/normal
        /// context. Most callers use the builder pattern in
        /// `crate::execute::build_spec` (Task 9); this is for tests.
        #[must_use]
        pub fn new(
            group: impl Into<String>,
            command: impl Into<String>,
            module: impl Into<String>,
            function: impl Into<String>,
            repo_root: impl Into<String>,
        ) -> Self {
            Self {
                schema_version: RUNNER_SCHEMA_VERSION,
                group: group.into(),
                command: command.into(),
                module: module.into(),
                function: function.into(),
                args: BTreeMap::new(),
                context: ContextSpec {
                    repo_root: repo_root.into(),
                    verbosity: "normal".into(),
                    timestamps: false,
                    log_level: "INFO".into(),
                },
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn spec_round_trips_through_json() {
            let mut spec = ExecutionSpec::new("ci", "hello", "tools.ci", "hello", "/repo");
            spec.args
                .insert("name".into(), serde_json::Value::String("Alice".into()));
            let json = serde_json::to_string(&spec).expect("serialize");
            let back: ExecutionSpec = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(spec, back);
        }

        #[test]
        fn spec_json_uses_python_field_names() {
            let spec = ExecutionSpec::new("ci", "hello", "tools.ci", "hello", "/repo");
            let json = serde_json::to_string(&spec).expect("serialize");
            // These exact strings are what `toolr._runner.RunnerSpec` decodes.
            assert!(json.contains("\"schema_version\":1"), "got: {json}");
            assert!(json.contains("\"group\":\"ci\""), "got: {json}");
            assert!(json.contains("\"command\":\"hello\""), "got: {json}");
            assert!(json.contains("\"repo_root\":\"/repo\""), "got: {json}");
            assert!(json.contains("\"verbosity\":\"normal\""), "got: {json}");
        }

        #[test]
        fn schema_version_constant_is_1() {
            assert_eq!(RUNNER_SCHEMA_VERSION, 1);
        }
    }
    ```

- [ ] **Step 4.4: Run the tests**

    ```bash
    cargo test --lib execute::
    ```

    Expected: 3 tests passing.

- [ ] **Step 4.5: Commit**

    ```bash
    git add src/lib.rs src/execute/
    git commit -m "feat(execute): Add ExecutionSpec serde types matching runner schema"
    ```

---

## Task 5: Tempfile-backed spec writer

Implement `write_spec_to_tempfile(&ExecutionSpec) -> io::Result<NamedTempFile>`
so the spec is written to a private temporary file that is auto-deleted when
the returned handle is dropped (including on panic).

**Files:**

- Create: `src/execute/tempfile.rs`
- Modify: `src/execute/mod.rs`

- [ ] **Step 5.1: Add `tempfile` to non-dev `[dependencies]` in `Cargo.toml`**

    Locate the `[dependencies]` table and add (alongside `serde_json`,
    `anyhow`, etc.):

    ```toml
    tempfile = ">=3.20"
    ```

    The existing `[dev-dependencies]` line for tempfile can stay — duplicate
    listings are merged by Cargo with the runtime entry taking precedence.

- [ ] **Step 5.2: Create `src/execute/tempfile.rs`**

    ```rust
    //! Write an [`ExecutionSpec`] to a private tempfile that auto-deletes
    //! when the returned handle is dropped (including on panic).

    use std::io::{self, Write};

    use tempfile::{Builder, NamedTempFile};

    use super::spec::ExecutionSpec;

    /// Write `spec` to a fresh tempfile and return its handle. The caller
    /// must keep the handle alive for as long as the path is needed —
    /// dropping it deletes the file.
    pub fn write_spec_to_tempfile(spec: &ExecutionSpec) -> io::Result<NamedTempFile> {
        let mut file = Builder::new()
            .prefix("toolr-spec-")
            .suffix(".json")
            .rand_bytes(12)
            .tempfile()?;
        let bytes = serde_json::to_vec(spec).map_err(io::Error::other)?;
        file.write_all(&bytes)?;
        file.flush()?;
        Ok(file)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::execute::spec::{ContextSpec, ExecutionSpec, RUNNER_SCHEMA_VERSION};
        use std::collections::BTreeMap;
        use std::fs;

        fn sample_spec() -> ExecutionSpec {
            ExecutionSpec {
                schema_version: RUNNER_SCHEMA_VERSION,
                group: "ci".into(),
                command: "hello".into(),
                module: "tools.ci".into(),
                function: "hello".into(),
                args: BTreeMap::new(),
                context: ContextSpec {
                    repo_root: "/repo".into(),
                    verbosity: "normal".into(),
                    timestamps: false,
                    log_level: "INFO".into(),
                },
            }
        }

        #[test]
        fn writes_valid_json_to_disk() {
            let spec = sample_spec();
            let file = write_spec_to_tempfile(&spec).expect("write");
            let read_back: ExecutionSpec =
                serde_json::from_slice(&fs::read(file.path()).unwrap()).expect("parse");
            assert_eq!(spec, read_back);
        }

        #[test]
        fn tempfile_path_has_expected_prefix_and_suffix() {
            let spec = sample_spec();
            let file = write_spec_to_tempfile(&spec).expect("write");
            let name = file
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            assert!(name.starts_with("toolr-spec-"), "name was {name}");
            assert!(name.ends_with(".json"), "name was {name}");
        }

        #[test]
        fn tempfile_is_deleted_on_drop() {
            let spec = sample_spec();
            let file = write_spec_to_tempfile(&spec).expect("write");
            let path = file.path().to_path_buf();
            assert!(path.exists());
            drop(file);
            assert!(!path.exists(), "tempfile should be gone after drop");
        }
    }
    ```

- [ ] **Step 5.3: Re-export from `src/execute/mod.rs`**

    Replace `src/execute/mod.rs`:

    ```rust
    //! Subprocess execution of user commands via `python -m toolr._runner`.

    pub mod spec;
    pub mod tempfile;

    pub use spec::{ContextSpec, ExecutionSpec, RUNNER_SCHEMA_VERSION};
    pub use tempfile::write_spec_to_tempfile;
    ```

- [ ] **Step 5.4: Run the tests**

    ```bash
    cargo test --lib execute::tempfile::
    ```

    Expected: 3 tests passing.

- [ ] **Step 5.5: Commit**

    ```bash
    git add Cargo.toml src/execute/
    git commit -m "feat(execute): Write spec JSON to private tempfile with drop-cleanup"
    ```

---

## Task 6: Python interpreter discovery

For Plan 2 we deliberately ship the simplest possible discovery — Plan 3
replaces this with proper tools-venv resolution. Order: `TOOLR_PYTHON` env
override → `python3` on PATH → `python` on PATH. Returns a typed error if
none can be found.

**Files:**

- Create: `src/execute/python.rs`
- Modify: `src/execute/mod.rs`

- [ ] **Step 6.1: Create `src/execute/python.rs`**

    ```rust
    //! Resolve a Python interpreter to use for `python -m toolr._runner`.
    //!
    //! Plan 2 ships the minimal viable lookup. Plan 3 replaces this with a
    //! resolved tools-venv interpreter under `<venv>/bin/python`.

    use std::env;
    use std::path::PathBuf;
    use std::process::Command;

    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum PythonError {
        #[error("no Python interpreter found. Set TOOLR_PYTHON or install python3 on PATH")]
        NotFound,
    }

    /// Resolve a Python interpreter, in priority order:
    ///
    /// 1. `$TOOLR_PYTHON` if set.
    /// 2. `python3` on PATH.
    /// 3. `python` on PATH.
    pub fn resolve_python() -> Result<PathBuf, PythonError> {
        if let Ok(p) = env::var("TOOLR_PYTHON") {
            if !p.is_empty() {
                return Ok(PathBuf::from(p));
            }
        }
        for candidate in ["python3", "python"] {
            if which_on_path(candidate).is_some() {
                return Ok(PathBuf::from(candidate));
            }
        }
        Err(PythonError::NotFound)
    }

    /// Cheap PATH check: spawn `<exe> --version` and see if it runs.
    /// (We avoid a `which` crate dep for one call site.)
    fn which_on_path(exe: &str) -> Option<()> {
        Command::new(exe)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .stdin(std::process::Stdio::null())
            .status()
            .ok()
            .filter(|s| s.success())
            .map(|_| ())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn toolr_python_env_var_wins() {
            // SAFETY: tests run in-process; we restore after.
            // SAFETY: std::env::set_var is single-threaded-safe inside a #[test]
            // when no other thread touches the environment. This crate's tests
            // don't spawn threads that touch env.
            unsafe {
                env::set_var("TOOLR_PYTHON", "/custom/python");
            }
            let p = resolve_python().expect("should resolve");
            assert_eq!(p, PathBuf::from("/custom/python"));
            unsafe {
                env::remove_var("TOOLR_PYTHON");
            }
        }

        #[test]
        fn falls_back_to_path_when_env_unset() {
            unsafe {
                env::remove_var("TOOLR_PYTHON");
            }
            // We can't assert a specific path without making the test brittle.
            // We only check that *if* python3/python is available, we get
            // a non-empty path back, *or* we get NotFound.
            match resolve_python() {
                Ok(p) => assert!(!p.as_os_str().is_empty()),
                Err(PythonError::NotFound) => {
                    // Acceptable on systems without any python on PATH.
                }
            }
        }
    }
    ```

    > Note: the `unsafe { env::set_var(...) }` form is required on Rust
    > 1.80+ where `set_var` was reclassified as `unsafe`. If the crate's
    > MSRV is earlier, drop the `unsafe` blocks.

- [ ] **Step 6.2: Re-export from `src/execute/mod.rs`**

    Replace `src/execute/mod.rs`:

    ```rust
    //! Subprocess execution of user commands via `python -m toolr._runner`.

    pub mod python;
    pub mod spec;
    pub mod tempfile;

    pub use python::{PythonError, resolve_python};
    pub use spec::{ContextSpec, ExecutionSpec, RUNNER_SCHEMA_VERSION};
    pub use tempfile::write_spec_to_tempfile;
    ```

- [ ] **Step 6.3: Run the tests**

    ```bash
    cargo test --lib execute::python::
    ```

    Expected: 2 tests passing.

- [ ] **Step 6.4: Commit**

    ```bash
    git add src/execute/
    git commit -m "feat(execute): Minimal python interpreter resolver (TOOLR_PYTHON or PATH)"
    ```

---

## Task 7: Subprocess spawn helper

Wrap `Command::new(python).arg("-m").arg("toolr._runner").env("TOOLR_SPEC_FILE", &path)`
with stdio inheritance. Return the child handle. No signal forwarding yet —
that's Task 10.

**Files:**

- Create: `src/execute/spawn.rs`
- Modify: `src/execute/mod.rs`

- [ ] **Step 7.1: Create `src/execute/spawn.rs`**

    ```rust
    //! Spawn the Python runner subprocess.

    use std::io;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};

    /// Spawn `<python> -m toolr._runner` with:
    ///
    /// - `TOOLR_SPEC_FILE` set to `spec_path`.
    /// - stdin/stdout/stderr inherited untouched (so Rich's TTY detection,
    ///   tools that read stdin, etc., all work).
    pub fn spawn_runner(python: &Path, spec_path: &Path) -> io::Result<Child> {
        Command::new(python)
            .arg("-m")
            .arg("toolr._runner")
            .env("TOOLR_SPEC_FILE", spec_path)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::path::PathBuf;

        #[test]
        fn spawn_with_nonexistent_python_returns_io_error() {
            let bogus = PathBuf::from("/definitely/not/a/real/python-binary-xyz");
            let result = spawn_runner(&bogus, Path::new("/tmp/whatever.json"));
            assert!(result.is_err());
        }
    }
    ```

- [ ] **Step 7.2: Re-export from `src/execute/mod.rs`**

    Replace `src/execute/mod.rs`:

    ```rust
    //! Subprocess execution of user commands via `python -m toolr._runner`.

    pub mod python;
    pub mod spawn;
    pub mod spec;
    pub mod tempfile;

    pub use python::{PythonError, resolve_python};
    pub use spawn::spawn_runner;
    pub use spec::{ContextSpec, ExecutionSpec, RUNNER_SCHEMA_VERSION};
    pub use tempfile::write_spec_to_tempfile;
    ```

- [ ] **Step 7.3: Run the test**

    ```bash
    cargo test --lib execute::spawn::
    ```

    Expected: 1 test passing.

- [ ] **Step 7.4: Commit**

    ```bash
    git add src/execute/
    git commit -m "feat(execute): Spawn python -m toolr._runner with inherited stdio"
    ```

---

## Task 8: Build an `ExecutionSpec` from a clap `ArgMatches`

Translate parsed clap matches for a known `Command` into an `ExecutionSpec`.
This is the bridge between Plan 1's clap tree and Plan 2's runner shim.

**Files:**

- Create: `src/execute/build.rs`
- Modify: `src/execute/mod.rs`

- [ ] **Step 8.1: Create `src/execute/build.rs`**

    ```rust
    //! Translate a parsed [`clap::ArgMatches`] into an [`ExecutionSpec`].

    use std::collections::BTreeMap;
    use std::path::Path;

    use clap::ArgMatches;
    use serde_json::Value;

    use crate::manifest::{Argument, ArgumentKind, Command};

    use super::spec::{ContextSpec, ExecutionSpec, RUNNER_SCHEMA_VERSION};

    /// Build the spec to write to disk, given:
    ///
    /// - `cmd`: the matched manifest command (already located by `dispatch`).
    /// - `matches`: clap's parsed matches *for this command* (not the root).
    /// - `repo_root`: the project root previously resolved by
    ///   `discover_project_root`.
    /// - `verbosity` / `timestamps` / `log_level`: pulled from the global CLI
    ///   args by the caller.
    pub fn build_spec(
        cmd: &Command,
        matches: &ArgMatches,
        repo_root: &Path,
        verbosity: &str,
        timestamps: bool,
        log_level: &str,
    ) -> ExecutionSpec {
        let mut args = BTreeMap::new();
        for arg in &cmd.arguments {
            if let Some(value) = extract_value(arg, matches) {
                args.insert(arg.name.clone(), value);
            }
        }
        ExecutionSpec {
            schema_version: RUNNER_SCHEMA_VERSION,
            group: cmd.group.clone(),
            command: cmd.name.clone(),
            module: cmd.module.clone(),
            function: cmd.function.clone(),
            args,
            context: ContextSpec {
                repo_root: repo_root.to_string_lossy().into_owned(),
                verbosity: verbosity.to_string(),
                timestamps,
                log_level: log_level.to_string(),
            },
        }
    }

    fn extract_value(arg: &Argument, matches: &ArgMatches) -> Option<Value> {
        match arg.kind {
            ArgumentKind::Flag => {
                // clap stored as bool via ArgAction::SetTrue.
                let v = matches.get_flag(arg.name.as_str());
                Some(Value::Bool(v))
            }
            ArgumentKind::Positional | ArgumentKind::Optional => matches
                .get_one::<String>(arg.name.as_str())
                .map(|s| Value::String(s.clone())),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::manifest::{Argument, ArgumentKind, Command, Origin};
        use clap::{Arg, ArgAction};

        fn cmd_hello_with_name_arg() -> Command {
            Command {
                name: "hello".into(),
                group: "ci".into(),
                module: "tools.ci".into(),
                function: "hello".into(),
                summary: "".into(),
                description: "".into(),
                arguments: vec![Argument {
                    name: "name".into(),
                    kind: ArgumentKind::Optional,
                    help: "".into(),
                    default: Some("world".into()),
                    type_annotation: None,
                    allowed_values: vec![],
                }],
                imports: vec![],
                origin: Origin::Static,
            }
        }

        fn parse(value: &str) -> ArgMatches {
            clap::Command::new("hello")
                .arg(
                    Arg::new("name")
                        .long("name")
                        .default_value("world"),
                )
                .get_matches_from(["hello", "--name", value])
        }

        #[test]
        fn build_spec_copies_static_fields() {
            let cmd = cmd_hello_with_name_arg();
            let matches = parse("Alice");
            let spec = build_spec(
                &cmd,
                &matches,
                Path::new("/repo"),
                "normal",
                false,
                "INFO",
            );
            assert_eq!(spec.group, "ci");
            assert_eq!(spec.command, "hello");
            assert_eq!(spec.module, "tools.ci");
            assert_eq!(spec.function, "hello");
            assert_eq!(spec.context.repo_root, "/repo");
        }

        #[test]
        fn build_spec_extracts_optional_arg_value() {
            let cmd = cmd_hello_with_name_arg();
            let matches = parse("Alice");
            let spec = build_spec(
                &cmd,
                &matches,
                Path::new("/repo"),
                "normal",
                false,
                "INFO",
            );
            assert_eq!(spec.args.get("name"), Some(&Value::String("Alice".into())));
        }

        #[test]
        fn build_spec_extracts_flag_as_bool() {
            let cmd = Command {
                name: "switch".into(),
                group: "ci".into(),
                module: "tools.ci".into(),
                function: "switch".into(),
                summary: "".into(),
                description: "".into(),
                arguments: vec![Argument {
                    name: "force".into(),
                    kind: ArgumentKind::Flag,
                    help: "".into(),
                    default: None,
                    type_annotation: None,
                    allowed_values: vec![],
                }],
                imports: vec![],
                origin: Origin::Static,
            };
            let matches = clap::Command::new("switch")
                .arg(Arg::new("force").long("force").action(ArgAction::SetTrue))
                .get_matches_from(["switch", "--force"]);
            let spec = build_spec(
                &cmd,
                &matches,
                Path::new("/repo"),
                "normal",
                false,
                "INFO",
            );
            assert_eq!(spec.args.get("force"), Some(&Value::Bool(true)));
        }
    }
    ```

- [ ] **Step 8.2: Re-export from `src/execute/mod.rs`**

    Replace `src/execute/mod.rs`:

    ```rust
    //! Subprocess execution of user commands via `python -m toolr._runner`.

    pub mod build;
    pub mod python;
    pub mod spawn;
    pub mod spec;
    pub mod tempfile;

    pub use build::build_spec;
    pub use python::{PythonError, resolve_python};
    pub use spawn::spawn_runner;
    pub use spec::{ContextSpec, ExecutionSpec, RUNNER_SCHEMA_VERSION};
    pub use tempfile::write_spec_to_tempfile;
    ```

- [ ] **Step 8.3: Run the tests**

    ```bash
    cargo test --lib execute::build::
    ```

    Expected: 3 tests passing.

- [ ] **Step 8.4: Commit**

    ```bash
    git add src/execute/
    git commit -m "feat(execute): Translate clap ArgMatches into ExecutionSpec"
    ```

---

## Task 9: Wire execution into `dispatch.rs`

Replace the exit-64 stub in `src/bin/toolr/dispatch.rs` with the full
pipeline: build spec → write tempfile → resolve python → spawn → wait →
propagate exit code.

**Files:**

- Modify: `src/bin/toolr/dispatch.rs`
- Modify: `src/bin/toolr/main.rs`

- [ ] **Step 9.1: Update `dispatch.rs` to call into `execute`**

    Replace `src/bin/toolr/dispatch.rs`:

    ```rust
    use std::process::ExitCode;

    use clap::ArgMatches;

    use _rust_utils::discovery::discover_project_root;
    use _rust_utils::execute::{
        build_spec, resolve_python, spawn_runner, write_spec_to_tempfile,
    };
    use _rust_utils::manifest::Manifest;

    pub fn dispatch(
        matches: &ArgMatches,
        manifest: &Manifest,
        root: &mut clap::Command,
    ) -> anyhow::Result<ExitCode> {
        if let Some(("__build-static-manifest", _)) = matches.subcommand() {
            return run_build_static_manifest();
        }
        let Some((group_name, group_matches)) = matches.subcommand() else {
            root.print_help()?;
            return Ok(ExitCode::SUCCESS);
        };
        let Some((cmd_name, cmd_matches)) = group_matches.subcommand() else {
            // toolr <group> with no command → print group help
            return Ok(ExitCode::SUCCESS);
        };
        let cmd = manifest
            .commands
            .iter()
            .find(|c| c.group == group_name && c.name == cmd_name)
            .ok_or_else(|| anyhow::anyhow!("unknown command: {group_name} {cmd_name}"))?;

        let cwd = std::env::current_dir()?;
        let repo_root = discover_project_root(&cwd)?;
        let verbosity = if matches.get_flag("quiet") {
            "quiet"
        } else if matches.get_flag("debug") {
            "verbose"
        } else {
            "normal"
        };
        let log_level = if matches.get_flag("debug") {
            "DEBUG"
        } else {
            "INFO"
        };
        let spec = build_spec(cmd, cmd_matches, &repo_root, verbosity, false, log_level);

        let tempfile = write_spec_to_tempfile(&spec)?;
        let python = resolve_python()?;
        let mut child = spawn_runner(&python, tempfile.path())?;
        let status = child.wait()?;

        // Map child status to a process exit code.
        let code = status.code().unwrap_or_else(|| {
            // Signal-terminated child on Unix: report 128 + signal.
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                if let Some(sig) = status.signal() {
                    return 128 + sig;
                }
            }
            1
        });
        // ExitCode only carries u8 — clamp anything outside 0..=255.
        let clamped: u8 = code.clamp(0, 255).try_into().unwrap_or(1);
        Ok(ExitCode::from(clamped))
    }

    fn run_build_static_manifest() -> anyhow::Result<ExitCode> {
        let cwd = std::env::current_dir()?;
        let root = _rust_utils::discovery::discover_project_root(&cwd)?;
        let tools = root.join("tools");
        let manifest = _rust_utils::parser::build_static_manifest(&tools)?;
        let path = tools.join(".toolr-manifest.json");
        _rust_utils::manifest::write_manifest(&path, &manifest)?;
        println!(
            "toolr: wrote {} groups / {} commands to {}",
            manifest.groups.len(),
            manifest.commands.len(),
            path.display()
        );
        Ok(ExitCode::SUCCESS)
    }
    ```

- [ ] **Step 9.2: Update the existing `running_a_user_command_emits_not_implemented_stub` test**

    The test in `tests/cli_smoke.rs` from Plan 1 asserts exit code 64 and
    "execution backend not yet implemented" in stderr. That's the stub
    we're now replacing. Delete that test (or convert it into an end-to-end
    smoke — Task 9.3 below adds a proper one).

    Open `tests/cli_smoke.rs` and remove the
    `running_a_user_command_emits_not_implemented_stub` function in its
    entirety, including its `#[test]` attribute.

- [ ] **Step 9.3: Add an end-to-end execution smoke test**

    Append to `tests/cli_smoke.rs`:

    ```rust
    use std::path::PathBuf;

    /// Returns `Some(path-to-python)` if a Python with msgspec + the local
    /// `toolr` package installed is available, otherwise `None`. We accept
    /// the project's own dev venv (created by `uv sync`) as the runner — full
    /// tools-venv resolution is Plan 3's job.
    fn detect_test_python() -> Option<PathBuf> {
        let candidate = std::env::var_os("TOOLR_TEST_PYTHON").map(PathBuf::from);
        let candidate = candidate.or_else(|| {
            // Project dev venv from `uv sync`.
            let p = PathBuf::from(".venv/bin/python");
            if p.exists() { Some(p) } else { None }
        });
        let python = candidate?;
        // Verify it can import `toolr._runner`. If not, skip.
        let status = std::process::Command::new(&python)
            .args(["-c", "import toolr._runner"])
            .status()
            .ok()?;
        if status.success() { Some(python) } else { None }
    }

    fn write_tools_demo(repo_root: &std::path::Path) {
        let tools_dir = repo_root.join("tools");
        std::fs::create_dir_all(&tools_dir).unwrap();
        std::fs::write(tools_dir.join("__init__.py"), "").unwrap();
        std::fs::write(
            tools_dir.join("demo.py"),
            r#"
    from toolr import command_group

    group = command_group("demo", "Demo", description="demo group")

    @group.command
    def hello(ctx, name: str = "world") -> None:
        ctx.print(f"hi {name}")
    "#,
        )
        .unwrap();
        let manifest = r#"{
            "schema_version": 1, "static_hash": "h", "dynamic_hash": "",
            "groups": [{"name": "demo", "title": "Demo", "description": "", "origin": "static"}],
            "commands": [{
                "name": "hello", "group": "demo", "module": "tools.demo",
                "function": "hello", "summary": "", "description": "",
                "arguments": [
                    {
                        "name": "name", "kind": "optional", "help": "",
                        "default": "world", "type_annotation": "str",
                        "allowed_values": []
                    }
                ],
                "imports": [], "origin": "static"
            }]
        }"#;
        std::fs::write(tools_dir.join(".toolr-manifest.json"), manifest).unwrap();
    }

    #[test]
    fn running_a_user_command_invokes_python_runner() {
        let Some(python) = detect_test_python() else {
            eprintln!(
                "skipping: no .venv/bin/python with toolr installed. \
                 Run `uv sync` first, or set TOOLR_TEST_PYTHON to a python \
                 that can `import toolr._runner`. Plan 3 will remove this \
                 skip by managing the tools venv automatically."
            );
            return;
        };
        let tmp = TempDir::new().unwrap();
        write_tools_demo(tmp.path());
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .env("TOOLR_PYTHON", &python)
            .args(["demo", "hello", "--name", "Alice"])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "expected success, got code {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
            output.status.code()
        );
        assert!(stdout.contains("hi Alice"), "stdout was:\n{stdout}");
    }

    #[test]
    fn user_command_propagates_nonzero_exit() {
        let Some(python) = detect_test_python() else {
            eprintln!("skipping: no test python (see above)");
            return;
        };
        let tmp = TempDir::new().unwrap();
        let tools_dir = tmp.path().join("tools");
        std::fs::create_dir_all(&tools_dir).unwrap();
        std::fs::write(tools_dir.join("__init__.py"), "").unwrap();
        std::fs::write(
            tools_dir.join("demo.py"),
            r#"
    from toolr import command_group

    group = command_group("demo", "Demo", description="demo group")

    @group.command
    def boom(ctx) -> None:
        ctx.exit(7, "failing on purpose")
    "#,
        )
        .unwrap();
        let manifest = r#"{
            "schema_version": 1, "static_hash": "h", "dynamic_hash": "",
            "groups": [{"name": "demo", "title": "Demo", "description": "", "origin": "static"}],
            "commands": [{
                "name": "boom", "group": "demo", "module": "tools.demo",
                "function": "boom", "summary": "", "description": "",
                "arguments": [], "imports": [], "origin": "static"
            }]
        }"#;
        std::fs::write(tools_dir.join(".toolr-manifest.json"), manifest).unwrap();
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .env("TOOLR_PYTHON", &python)
            .args(["demo", "boom"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(7));
    }
    ```

- [ ] **Step 9.4: Run the smoke tests**

    ```bash
    cargo test --test cli_smoke
    ```

    Expected: all tests passing (the new end-to-end tests will skip with a
    clear message if `.venv/bin/python` isn't available, otherwise they
    drive the full pipeline).

- [ ] **Step 9.5: Manual smoke against the real repo**

    ```bash
    uv sync
    cargo build --bin toolr --release
    ./target/release/toolr __build-static-manifest
    ./target/release/toolr --help
    # Pick any real command from tools/ci.py:
    TOOLR_PYTHON=$PWD/.venv/bin/python ./target/release/toolr ci --help
    ```

    Expected: `--help` shows the same groups/commands as before, and invoking
    a real command runs the Python function instead of printing the "execution
    backend not yet implemented" stub.

- [ ] **Step 9.6: Commit**

    ```bash
    git add src/bin/toolr/dispatch.rs tests/cli_smoke.rs
    git commit -m "feat(execute): Wire Python runner into dispatch with exit-code propagation"
    ```

---

## Task 10: Forward SIGINT/SIGTERM to the child

When the toolr binary receives SIGINT (Ctrl+C) or SIGTERM, forward it to the
Python subprocess instead of dying immediately and orphaning the child.

**Files:**

- Create: `src/execute/signals.rs`
- Modify: `src/execute/mod.rs`
- Modify: `Cargo.toml`
- Modify: `src/bin/toolr/dispatch.rs`

- [ ] **Step 10.1: Add `signal-hook` to `[dependencies]` in `Cargo.toml`**

    ```toml
    signal-hook = "0.3"
    ```

- [ ] **Step 10.2: Create `src/execute/signals.rs`**

    ```rust
    //! Forward SIGINT and SIGTERM from the parent toolr process to the
    //! Python runner subprocess.
    //!
    //! Strategy: register signal handlers that write the received signal
    //! number to an `Arc<AtomicI32>`. A polling loop in [`wait_with_signals`]
    //! checks the atomic between waits and re-sends the signal to the child
    //! pid.
    //!
    //! On Windows we install Ctrl-C handling only — SIGTERM does not exist
    //! and Ctrl-C is propagated to the child by the console subsystem by
    //! default, so this is effectively a no-op there.

    use std::io;
    use std::process::{Child, ExitStatus};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::time::Duration;

    /// Wait for `child` to exit, forwarding SIGINT/SIGTERM received by the
    /// current process to the child along the way.
    pub fn wait_with_signals(child: &mut Child) -> io::Result<ExitStatus> {
        #[cfg(unix)]
        {
            unix::wait_with_signals(child)
        }
        #[cfg(not(unix))]
        {
            // No portable signal forwarding outside Unix. Just wait.
            let _ = (Arc::new(AtomicI32::new(0)), Duration::from_millis(0));
            child.wait()
        }
    }

    #[cfg(unix)]
    mod unix {
        use super::*;

        use signal_hook::consts::{SIGINT, SIGTERM};
        use signal_hook::iterator::Signals;

        pub fn wait_with_signals(child: &mut Child) -> io::Result<ExitStatus> {
            let pending = Arc::new(AtomicI32::new(0));
            let mut signals = Signals::new([SIGINT, SIGTERM])?;
            let pending_for_thread = Arc::clone(&pending);
            let handle = signals.handle();

            let listener = std::thread::spawn(move || {
                for sig in &mut signals {
                    pending_for_thread.store(sig, Ordering::SeqCst);
                }
            });

            let child_pid = child.id() as i32;
            let status = loop {
                if let Some(status) = child.try_wait()? {
                    break status;
                }
                let sig = pending.swap(0, Ordering::SeqCst);
                if sig != 0 {
                    // Re-send to the child. Ignore errors (the child may have
                    // just exited).
                    // SAFETY: `kill` is a libc FFI call with no preconditions
                    // beyond the pid being a valid signed int.
                    unsafe {
                        libc::kill(child_pid, sig);
                    }
                }
                std::thread::sleep(Duration::from_millis(50));
            };

            handle.close();
            let _ = listener.join();
            Ok(status)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::process::Command;

        #[test]
        fn wait_returns_for_quick_child() {
            // `true` exits immediately with status 0. On Windows this won't
            // exist; the test is Unix-only.
            #[cfg(unix)]
            {
                let mut child = Command::new("true").spawn().expect("spawn true");
                let status = wait_with_signals(&mut child).expect("wait");
                assert!(status.success());
            }
        }
    }
    ```

- [ ] **Step 10.3: Re-export from `src/execute/mod.rs`**

    Replace `src/execute/mod.rs`:

    ```rust
    //! Subprocess execution of user commands via `python -m toolr._runner`.

    pub mod build;
    pub mod python;
    pub mod signals;
    pub mod spawn;
    pub mod spec;
    pub mod tempfile;

    pub use build::build_spec;
    pub use python::{PythonError, resolve_python};
    pub use signals::wait_with_signals;
    pub use spawn::spawn_runner;
    pub use spec::{ContextSpec, ExecutionSpec, RUNNER_SCHEMA_VERSION};
    pub use tempfile::write_spec_to_tempfile;
    ```

- [ ] **Step 10.4: Use `wait_with_signals` from `dispatch.rs`**

    In `src/bin/toolr/dispatch.rs`, update the import:

    ```rust
    use _rust_utils::execute::{
        build_spec, resolve_python, spawn_runner, wait_with_signals, write_spec_to_tempfile,
    };
    ```

    And replace the line `let status = child.wait()?;` with:

    ```rust
    let status = wait_with_signals(&mut child)?;
    ```

- [ ] **Step 10.5: Run the tests**

    ```bash
    cargo test --lib execute::signals::
    cargo test --test cli_smoke
    ```

    Expected: signal test passes on Unix; smoke tests still pass.

- [ ] **Step 10.6: Manual interactive smoke (optional)**

    ```bash
    cargo build --bin toolr --release
    # In one terminal, start a long-running test tool. In another, send
    # SIGINT and confirm both the toolr binary and the Python child exit.
    # (Skip if no suitable long-running command is available in tools/.)
    ```

- [ ] **Step 10.7: Commit**

    ```bash
    git add Cargo.toml src/execute/ src/bin/toolr/dispatch.rs
    git commit -m "feat(execute): Forward SIGINT/SIGTERM to Python runner subprocess"
    ```

---

## Task 11: Make `_runner` discoverable from the `toolr` package surface

The runner shim is loaded via `python -m toolr._runner`, which only requires
that the module file lives under the `toolr` package — Python's import system
finds it without any explicit `__all__` entry. But we do want a sanity check
that the module is shipped and importable on a clean install, and a one-line
note in `__init__.py`'s top-of-file docstring.

**Files:**

- Modify: `python/toolr/__init__.py`
- Create: `tests/runner/test_module_shipped.py`

- [ ] **Step 11.1: Write a failing test in `tests/runner/test_module_shipped.py`**

    ```python
    """Sanity checks that ``toolr._runner`` ships with the package."""

    from __future__ import annotations

    import importlib
    import importlib.metadata
    import importlib.util


    def test_runner_module_is_importable() -> None:
        # Smoke: simply importing the module should not raise.
        mod = importlib.import_module("toolr._runner")
        assert hasattr(mod, "main")
        assert hasattr(mod, "RunnerSpec")
        assert mod.SCHEMA_VERSION == 1


    def test_runner_module_file_is_under_toolr_package() -> None:
        spec = importlib.util.find_spec("toolr._runner")
        assert spec is not None, "toolr._runner should be findable"
        assert spec.origin is not None
        # Reaching this assertion means the source file is shipped alongside
        # the rest of the package — installing the wheel ships it too.
        assert spec.origin.endswith("_runner.py")
    ```

- [ ] **Step 11.2: Run the tests**

    ```bash
    uv run pytest tests/runner/test_module_shipped.py -v
    ```

    Expected: both tests pass (the module from Tasks 1–3 already provides
    `main`, `RunnerSpec`, and `SCHEMA_VERSION`). If anything is missing this
    surfaces the gap; otherwise this test acts as a regression guard.

- [ ] **Step 11.3: Add a one-line note to `python/toolr/__init__.py`**

    Open `python/toolr/__init__.py` and confirm it does not need to import
    `_runner` (it shouldn't — the runner is invoked by `python -m`, not by
    `import toolr`). Add a brief docstring at the top of the file noting the
    runner's existence. The file currently starts with `from __future__ import
    annotations`; prepend:

    ```python
    """ToolR Python package.

    Importable surface: :class:`toolr.Context`, :func:`toolr.command_group`,
    :func:`toolr.arg`, :func:`toolr.report_on_import_errors`.

    Implementation modules (not part of the user-facing API):

    - ``toolr._runner``: invoked by the toolr binary via
      ``python -m toolr._runner``; reads ``$TOOLR_SPEC_FILE`` and dispatches
      into user code.
    """

    from __future__ import annotations
    ```

- [ ] **Step 11.4: Re-run tests to confirm no regressions**

    ```bash
    uv run pytest tests/runner/ -v
    ```

    Expected: all runner tests pass.

- [ ] **Step 11.5: Commit**

    ```bash
    git add python/toolr/__init__.py tests/runner/test_module_shipped.py
    git commit -m "docs(toolr): Note toolr._runner module + ship-test"
    ```

---

## Task 12: Update the roadmap

Mark Plan 2 as Done in the roadmap once everything above is merged.

**Files:**

- Modify: `specs/rust-front-end/01-roadmap.md`

- [ ] **Step 12.1: Update the Plan 2 entry**

    Locate the `### Plan 2: Python runner + execute model (S1)` block and
    change the **Status** line:

    ```markdown
    ### Plan 2: Python runner + execute model (S1)

    - **Status:** ✅ Done
    - **Plan doc:** [03-plan-2-runner-execute.md](./03-plan-2-runner-execute.md)
    - **Depends on:** Plan 1
    - **Unblocks:** Plans 3, 7
    - **Produces:**
        - …(unchanged)…
    ```

- [ ] **Step 12.2: Commit**

    ```bash
    git add specs/rust-front-end/01-roadmap.md
    git commit -m "docs(roadmap): Mark Plan 2 as done"
    ```

---

## Done criteria

Plan 2 is complete when:

- `cargo test` passes for all unit and integration tests in `_rust_utils`
  (including the new `execute::*` test modules and the updated
  `tests/cli_smoke.rs`).
- `uv run pytest tests/runner/` passes (4 test files: spec schema, loader,
  dispatch, module-shipped).
- `python -m toolr._runner` exits non-zero with a clear message when
  `TOOLR_SPEC_FILE` is not set.
- With a populated tools manifest and `.venv/bin/python` available (or
  `TOOLR_PYTHON` pointing at a python with the `toolr` package installed),
  invoking `toolr <group> <command> [args...]` runs the Python function and
  the user sees identical output to today's argparse-driven path.
- Non-zero exit codes from inside Python (`ctx.exit(7, ...)`) propagate all
  the way to the shell.
- The exit-64 "execution backend not yet implemented" stub from Plan 1's
  Task 15 is gone — the corresponding smoke test has been removed and the
  new end-to-end smokes pass.
- SIGINT delivered to the toolr binary while a tool is running terminates
  the Python child (verified manually; the automated test simply confirms
  `wait_with_signals` returns for a quick child).
- The roadmap status table reflects Plan 2 as `✅ Done`.

## Open questions (for the implementer)

1. **`signal-hook` vs. `ctrlc` vs. hand-rolled.** The plan uses `signal-hook`
   because it's the standard choice for SIGINT/SIGTERM forwarding in Rust
   CLI tools (it's what `uv`, `bacon`, `cargo-watch` use). If the implementer
   finds `signal-hook`'s thread-per-process model burdensome, `ctrlc` is a
   simpler one-callback alternative — but it doesn't cover SIGTERM. Stay
   with `signal-hook` unless there's a specific reason to switch.
2. **Windows signal handling.** Job objects on Windows would let us tear
   down the whole subtree on Ctrl+C without explicit forwarding. The
   stub-out in `signals.rs` (`#[cfg(not(unix))]`) is intentional —
   Windows-specific work is deferred until we actively support Windows in CI.
   File a follow-up if/when that becomes a priority.
3. **`TOOLR_PYTHON` lifecycle.** Plan 2 introduces `TOOLR_PYTHON` as a stop-gap
   so end-to-end tests and manual smokes work before Plan 3 lands. Plan 3
   replaces `resolve_python()` with a venv-aware resolver. The env var should
   remain as a documented escape hatch (useful for CI matrix testing across
   Python versions), but its priority over the venv-resolved interpreter is
   Plan 3's call.
4. **`ContextSpec` fields.** The current spec carries `repo_root`, `verbosity`,
   `timestamps`, and `log_level` — the minimum needed to reconstruct a
   `Context` that the existing `tools/*.py` code expects. If user tools
   reach into more `Context` attributes, those need to be added here and to
   `_build_context` in the runner. Audit `tools/ci.py` and `tools/version.py`
   during implementation; if anything is missing, expand `ContextSpec` and
   bump `SCHEMA_VERSION` to 2 with a matching update on the Python side.
5. **`args` JSON shape.** `build_spec` currently emits all clap-extracted
   values as strings (for positional/optional) or bools (for flags). The
   Python runner happily accepts these because the existing
   `tools/*.py` signatures use `str` annotations almost everywhere. If a
   command declares `count: int = 3`, the user's function will receive a
   string and need to coerce. This matches today's argparse behavior in
   `python/toolr/_parser.py` (which is going away). Cleaner type-aware
   conversion lives in Plan 6 (dynamic manifest) or as a future enhancement
   to the static parser — out of scope for Plan 2.
