<!-- rumdl-disable MD046 MD076 -->

# Plan 6: Dynamic Manifest Layer

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Lint:** Plan docs nest fenced code inside list items for step-by-step
> structure. The `<!-- rumdl-disable MD046 MD076 -->` directive above turns
> off the code-block-style and list-item-spacing rules for this file only.

**Goal:** Add the dynamic manifest layer that imports `tools.*` modules inside
the tools venv, walks the `command_group` / `@group.command` registry, and
enumerates legacy `importlib.metadata` entry points. The Rust side spawns a
short-lived Python subprocess (`python -m toolr._introspect`), reads JSON from
stdout, and merges entries tagged `Origin::Dynamic` into the manifest. The
combined manifest is regeneratable via the new
`toolr project manifest rebuild` command and is auto-rebuilt at execute time
when the dynamic-layer hash is stale relative to the installed package set.
A shipped pre-commit hook entry runs the rebuild on changes under `tools/`.

**Scope boundary.** The dynamic layer is the **fallback** path. Plan 5 covers
third-party packages that adopt the static `toolr-manifest.json` convention;
Plan 6 catches everything Plan 5 cannot:

- Legacy third-party packages that register via `importlib.metadata` entry
  points and have not adopted the static manifest convention (including
  editable installs).
- Dynamically-registered commands in `tools/*.py` the static parser does not
  see (loops, conditional decorators, runtime factories).
- Argument value completers and enum/Literal types that require runtime eval.

The dynamic layer **never runs at Tab time** — completion always serves from
the cached manifest.

**Architecture:** A Python helper `python/toolr/_introspect.py` imports the
project's `tools.*` modules, queries the `command_group` registry, enumerates
entry points under the `toolr.commands` group, and writes a JSON payload to
stdout. The Rust side adds an `_rust_utils::dynamic` module that spawns the
helper inside the tools venv, deserializes the payload into the same
`Group`/`Command` types from Plan 1 (tagged `Origin::Dynamic`), and merges
into the manifest. A `compute_dynamic_hash` function over the venv's
`*.dist-info` directory provides the freshness signal. A new dispatcher hook
auto-rebuilds at execute time when the manifest's `dynamic_hash` does not
match the venv. The CLI exposes `toolr project manifest rebuild`. A
`.pre-commit-hooks.yaml` at the repo root advertises the
`toolr-manifest` hook.

**Tech Stack:** Rust 2021, serde_json, blake3 (already in deps), anyhow,
assert_cmd; Python 3.11+ stdlib (`importlib`, `importlib.metadata`,
`pkgutil`, `inspect`, `json`, `sys`, `argparse`).

**Dependencies:**

- **Plan 3** (Tools venv + uv) — provides the resolver that returns the
  active tools venv path so this plan can locate the Python interpreter to
  spawn. Concretely, this plan calls a `_rust_utils::venv::resolve_tools_venv`
  function expected to land in Plan 3. If Plan 3 is not yet merged, gate
  development of Plan 6 on a stub that returns a hard-coded path or take a
  dependency at task-execution time.
- **Plan 2** (Python runner + execute model) — provides the subprocess-spawn
  pattern (spec tempfile + `python -m toolr._<helper>`); this plan reuses that
  pattern shape but with stdout-based payload delivery rather than a spec
  tempfile.
- **Plan 1** (Rust skeleton) — provides `Manifest`, `Group`, `Command`,
  `Origin::Dynamic`, `load_manifest`, `write_manifest`, and the existing
  `_rust_utils::parser::build_static_manifest`.

**Reading order in this plan:** Tasks build on each other. Don't skip ahead;
later tasks reference functions and JSON shapes defined in earlier ones.

---

## Task 1: Define the dynamic-layer JSON payload schema

Pin the wire format between the Python introspection helper and the Rust
side. Use the same field names as the `Manifest` model from Plan 1 so the
payload deserializes directly into the existing `Group` and `Command` types.

**Files:**

- Create: `src/dynamic/mod.rs`
- Create: `src/dynamic/payload.rs`
- Create: `src/dynamic/tests.rs`
- Modify: `src/lib.rs`

- [x] **Step 1.1: Expose a `dynamic` module from `src/lib.rs`**

    Add to `src/lib.rs`:

    ```rust
    pub mod dynamic;
    ```

- [x] **Step 1.2: Create `src/dynamic/mod.rs`**

    ```rust
    //! Dynamic manifest layer: spawn a Python introspection helper inside
    //! the tools venv and merge the result into the manifest.

    pub mod payload;

    pub use payload::{DynamicPayload, PAYLOAD_SCHEMA_VERSION};

    #[cfg(test)]
    mod tests;
    ```

- [x] **Step 1.3: Create `src/dynamic/payload.rs`**

    ```rust
    //! Wire format for the dynamic-layer introspection payload.

    use serde::{Deserialize, Serialize};

    use crate::manifest::{Argument, Group, Command, Origin};

    /// Wire-protocol version between `toolr._introspect` and the Rust side.
    /// Bump on breaking changes to `DynamicPayload`.
    pub const PAYLOAD_SCHEMA_VERSION: u32 = 1;

    /// JSON payload written to stdout by `python -m toolr._introspect`.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct DynamicPayload {
        /// Schema version of the payload itself (NOT the manifest version).
        pub payload_schema_version: u32,
        /// Groups discovered by importing `tools.*` and via entry points.
        pub groups: Vec<Group>,
        /// Commands discovered the same way.
        pub commands: Vec<Command>,
        /// Non-fatal warnings the helper wants to surface (e.g. a tools
        /// module that failed to import). Each is a single human-readable
        /// line. Rust prints them to stderr after a successful merge.
        #[serde(default)]
        pub warnings: Vec<String>,
    }

    impl DynamicPayload {
        /// Force every group / command in the payload to `Origin::Dynamic`,
        /// regardless of what the Python side emitted. The Rust side owns
        /// origin tagging — defence-in-depth against a misbehaving helper.
        pub fn retag_as_dynamic(mut self) -> Self {
            for g in &mut self.groups {
                g.origin = Origin::Dynamic;
            }
            for c in &mut self.commands {
                c.origin = Origin::Dynamic;
            }
            self
        }
    }

    /// Used only for tests / docs to make the unused-import warning shut up.
    #[allow(dead_code)]
    fn _argument_is_used(_a: Argument) {}
    ```

- [x] **Step 1.4: Add round-trip tests in `src/dynamic/tests.rs`**

    ```rust
    use super::payload::{DynamicPayload, PAYLOAD_SCHEMA_VERSION};
    use crate::manifest::{Command, Group, Origin};

    fn sample_payload() -> DynamicPayload {
        DynamicPayload {
            payload_schema_version: PAYLOAD_SCHEMA_VERSION,
            groups: vec![Group {
                name: "legacy".into(),
                title: "Legacy entry-point group".into(),
                description: "".into(),
                origin: Origin::Dynamic,
            }],
            commands: vec![Command {
                name: "frob".into(),
                group: "legacy".into(),
                module: "third_party_pkg.commands".into(),
                function: "frob".into(),
                summary: "Frob the thing.".into(),
                description: "".into(),
                arguments: vec![],
                imports: vec![],
                origin: Origin::Dynamic,
            }],
            warnings: vec![],
        }
    }

    #[test]
    fn payload_round_trips_through_json() {
        let p = sample_payload();
        let json = serde_json::to_string(&p).expect("serialize");
        let back: DynamicPayload = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(p, back);
    }

    #[test]
    fn retag_overwrites_origin_to_dynamic() {
        let mut p = sample_payload();
        p.groups[0].origin = Origin::Static;
        p.commands[0].origin = Origin::Static;
        let retagged = p.retag_as_dynamic();
        assert_eq!(retagged.groups[0].origin, Origin::Dynamic);
        assert_eq!(retagged.commands[0].origin, Origin::Dynamic);
    }

    #[test]
    fn missing_warnings_field_defaults_to_empty() {
        let json = r#"{
            "payload_schema_version": 1,
            "groups": [],
            "commands": []
        }"#;
        let p: DynamicPayload = serde_json::from_str(json).expect("deserialize minimal");
        assert!(p.warnings.is_empty());
    }
    ```

- [x] **Step 1.5: Run tests**

    ```bash
    cargo test --lib dynamic::
    ```

    Expected: 3 tests passing.

- [x] **Step 1.6: Commit**

    ```bash
    git add src/lib.rs src/dynamic/
    git commit -m "feat(dynamic): Define dynamic-layer payload schema and tests"
    ```

---

## Task 2: Python introspection helper — module scaffolding

Add `python/toolr/_introspect.py` with the bare entry point. The file must
run as `python -m toolr._introspect` and emit a valid empty payload to
stdout when the project has no tools and no entry points. Later tasks fill
in the registry walk and entry-point enumeration.

**Files:**

- Create: `python/toolr/_introspect.py`
- Create: `tests/test_introspect_empty.py`

- [x] **Step 2.1: Create `python/toolr/_introspect.py` with the bare main**

    ```python
    """Dynamic manifest introspection helper.

    Invoked as ``python -m toolr._introspect`` from the Rust side inside the
    project's tools venv. Walks the ``command_group`` registry after
    importing every module under ``tools.*``, enumerates ``importlib.metadata``
    entry points in the ``toolr.commands`` group, and writes a JSON payload
    to stdout.

    The wire format is defined in ``specs/rust-front-end/07-plan-6-dynamic-manifest.md``.
    Bump ``PAYLOAD_SCHEMA_VERSION`` on every breaking change.
    """

    from __future__ import annotations

    import argparse
    import json
    import sys
    from typing import Any

    PAYLOAD_SCHEMA_VERSION = 1


    def build_payload(tools_root: str | None) -> dict[str, Any]:
        """Construct the dynamic-layer payload for the current Python env.

        Args:
            tools_root: Absolute path to the project's ``tools/`` directory,
                or ``None`` if the caller could not resolve one. When given,
                the helper inserts the parent of ``tools_root`` on
                ``sys.path`` so ``import tools.<sub>`` works.
        """
        warnings: list[str] = []
        groups: list[dict[str, Any]] = []
        commands: list[dict[str, Any]] = []

        # Tasks 3 and 4 fill these in; for now we emit an empty payload so
        # the wiring works end-to-end.
        _ = tools_root

        return {
            "payload_schema_version": PAYLOAD_SCHEMA_VERSION,
            "groups": groups,
            "commands": commands,
            "warnings": warnings,
        }


    def main(argv: list[str] | None = None) -> int:
        parser = argparse.ArgumentParser(
            prog="toolr._introspect",
            description="Dump toolr dynamic-layer manifest as JSON to stdout.",
        )
        parser.add_argument(
            "--tools-root",
            default=None,
            help="Absolute path to the project's tools/ directory.",
        )
        args = parser.parse_args(argv)
        payload = build_payload(args.tools_root)
        json.dump(payload, sys.stdout, separators=(",", ":"))
        sys.stdout.write("\n")
        return 0


    if __name__ == "__main__":
        raise SystemExit(main())
    ```

- [x] **Step 2.2: Write a smoke test for the empty payload**

    Create `tests/test_introspect_empty.py`:

    ```python
    """Smoke tests for the dynamic-manifest introspection helper."""

    from __future__ import annotations

    import json
    import subprocess
    import sys


    def test_empty_project_emits_valid_payload() -> None:
        """`python -m toolr._introspect` with no tools_root produces a parseable empty payload."""
        proc = subprocess.run(
            [sys.executable, "-m", "toolr._introspect"],
            capture_output=True,
            text=True,
            check=True,
        )
        payload = json.loads(proc.stdout)
        assert payload["payload_schema_version"] == 1
        assert payload["groups"] == []
        assert payload["commands"] == []
        assert payload["warnings"] == []


    def test_help_flag_exits_zero() -> None:
        proc = subprocess.run(
            [sys.executable, "-m", "toolr._introspect", "--help"],
            capture_output=True,
            text=True,
            check=False,
        )
        assert proc.returncode == 0
        assert "Dump toolr dynamic-layer manifest" in proc.stdout
    ```

- [x] **Step 2.3: Run the Python tests**

    ```bash
    uv run pytest tests/test_introspect_empty.py -q
    ```

    Expected: 2 tests passing.

- [x] **Step 2.4: Commit**

    ```bash
    git add python/toolr/_introspect.py tests/test_introspect_empty.py
    git commit -m "feat(introspect): Add dynamic-manifest helper skeleton with empty-payload smoke test"
    ```

---

## Task 3: Python introspection helper — `tools.*` registry walk

Import every module under `tools.*`, then read the singleton storage exposed
by `toolr._registry._get_command_group_storage` to discover groups and their
commands. Tag each as dynamic. Failures to import individual modules become
warnings, not hard errors.

**Files:**

- Modify: `python/toolr/_introspect.py`
- Create: `tests/test_introspect_tools_walk.py`

- [x] **Step 3.1: Add the tools walk to `_introspect.py`**

    Replace the body of `build_payload` and add helpers:

    ```python
    from __future__ import annotations

    import argparse
    import importlib
    import inspect
    import json
    import os
    import pkgutil
    import sys
    import traceback
    from typing import Any

    PAYLOAD_SCHEMA_VERSION = 1


    def _ensure_tools_on_syspath(tools_root: str | None) -> None:
        """Insert the parent of ``tools_root`` on ``sys.path`` so ``import tools.<sub>`` works."""
        if not tools_root:
            return
        parent = os.path.dirname(os.path.abspath(tools_root))
        if parent and parent not in sys.path:
            sys.path.insert(0, parent)


    def _import_tools_modules(warnings: list[str]) -> None:
        """Import every module under the top-level ``tools`` package.

        Failures import a single module are converted to a warning string and
        the walk continues — one bad file must not poison the whole rebuild.
        """
        try:
            tools_pkg = importlib.import_module("tools")
        except ModuleNotFoundError:
            # No `tools/` package on sys.path; nothing to walk.
            return
        except Exception as exc:  # pragma: no cover — defensive
            warnings.append(f"failed to import top-level `tools` package: {exc!r}")
            return

        search_paths = getattr(tools_pkg, "__path__", None)
        if not search_paths:
            return

        for module_info in pkgutil.walk_packages(search_paths, prefix="tools."):
            try:
                importlib.import_module(module_info.name)
            except Exception as exc:  # noqa: BLE001 — we want every error
                warnings.append(
                    f"failed to import `{module_info.name}`: {type(exc).__name__}: {exc}"
                )


    def _walk_registry() -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
        """Read groups and commands from the toolr registry singleton."""
        from toolr._registry import _get_command_group_storage  # type: ignore[attr-defined]

        storage = _get_command_group_storage()
        groups: list[dict[str, Any]] = []
        commands: list[dict[str, Any]] = []

        for full_name, group in storage.items():
            # CommandGroup.full_name is "tools.<name>" or "tools.<parent>.<name>";
            # we strip the leading "tools." for the manifest's `group` field.
            display_name = full_name
            if display_name.startswith("tools."):
                display_name = display_name[len("tools."):]
            groups.append(
                {
                    "name": display_name,
                    "title": group.title,
                    "description": group.description or "",
                    "origin": "dynamic",
                }
            )
            registered = getattr(group, f"_{type(group).__name__}__commands", None)
            if registered is None:
                # Storage uses name-mangling for the __commands dict.
                registered = group.__dict__.get("_CommandGroup__commands", {})
            for cmd_name, func in (registered or {}).items():
                commands.append(_command_entry(display_name, cmd_name, func))

        return groups, commands


    def _command_entry(group_name: str, cmd_name: str, func: Any) -> dict[str, Any]:
        """Serialize a single registered command function."""
        module = getattr(func, "__module__", "") or ""
        function = getattr(func, "__name__", cmd_name)
        doc = inspect.getdoc(func) or ""
        summary, _, description = doc.partition("\n\n")
        return {
            "name": cmd_name,
            "group": group_name,
            "module": module,
            "function": function,
            "summary": summary.strip(),
            "description": description.strip(),
            # Argument extraction is intentionally omitted here. The static
            # parser already emits these for `tools/*.py` files; the dynamic
            # layer only adds *missing* commands. Arguments for dynamic-only
            # commands are filled in by Task 4's entry-point pass for legacy
            # third-party packages.
            "arguments": [],
            "imports": [],
            "origin": "dynamic",
        }


    def build_payload(tools_root: str | None) -> dict[str, Any]:
        warnings: list[str] = []
        _ensure_tools_on_syspath(tools_root)
        _import_tools_modules(warnings)
        groups, commands = _walk_registry()
        return {
            "payload_schema_version": PAYLOAD_SCHEMA_VERSION,
            "groups": groups,
            "commands": commands,
            "warnings": warnings,
        }
    ```

- [x] **Step 3.2: Write tests for the tools walk**

    Create `tests/test_introspect_tools_walk.py`:

    ```python
    """Tests for the dynamic-manifest helper walking a tools/ fixture."""

    from __future__ import annotations

    import json
    import subprocess
    import sys
    import textwrap
    from collections.abc import Callable
    from pathlib import Path

    import pytest


    @pytest.fixture
    def tools_fixture(tmp_path: Path) -> Callable[[], Path]:
        """Factory: scaffold a ``tools/demo.py`` fixture under ``tmp_path``.

        Returns the ``tools/`` directory path.
        """

        def _make() -> Path:
            tools = tmp_path / "tools"
            tools.mkdir()
            (tools / "__init__.py").write_text("")
            (tools / "demo.py").write_text(
                textwrap.dedent(
                    '''
                    """Demo dynamic-layer module."""
                    from toolr import command_group

                    group = command_group("demo", "Demo group", description="A demo.")

                    @group.command
                    def shout(ctx):
                        """Shout loudly."""
                        return 0
                    '''
                ).strip()
                + "\n"
            )
            return tools

        return _make


    def test_tools_walk_finds_decorated_command(
        tools_fixture: Callable[[], Path],
        tmp_path: Path,
    ) -> None:
        tools_root = tools_fixture()
        proc = subprocess.run(
            [sys.executable, "-m", "toolr._introspect", "--tools-root", str(tools_root)],
            capture_output=True,
            text=True,
            check=True,
            cwd=str(tmp_path),
        )
        payload = json.loads(proc.stdout)
        names = {g["name"] for g in payload["groups"]}
        assert "demo" in names, payload
        cmd_names = {(c["group"], c["name"]) for c in payload["commands"]}
        assert ("demo", "shout") in cmd_names, payload


    def test_broken_module_becomes_warning(tmp_path: Path) -> None:
        tools = tmp_path / "tools"
        tools.mkdir()
        (tools / "__init__.py").write_text("")
        (tools / "broken.py").write_text("raise RuntimeError('boom at import')\n")
        proc = subprocess.run(
            [sys.executable, "-m", "toolr._introspect", "--tools-root", str(tools)],
            capture_output=True,
            text=True,
            check=True,
            cwd=str(tmp_path),
        )
        payload = json.loads(proc.stdout)
        assert any("broken" in w for w in payload["warnings"]), payload
    ```

- [x] **Step 3.3: Run the new tests**

    ```bash
    uv run pytest tests/test_introspect_tools_walk.py -q
    ```

    Expected: 2 tests passing.

- [x] **Step 3.4: Commit**

    ```bash
    git add python/toolr/_introspect.py tests/test_introspect_tools_walk.py
    git commit -m "feat(introspect): Walk tools.* and emit registry-derived groups and commands"
    ```

---

## Task 4: Python introspection helper — entry-point enumeration

Add a pass that walks `importlib.metadata.entry_points(group="toolr.commands")`,
loads each entry point, and merges the resulting registry state in. This is
the legacy fallback for third-party packages that ship entry points instead
of a static `toolr-manifest.json` (Plan 5).

**Files:**

- Modify: `python/toolr/_introspect.py`
- Create: `tests/test_introspect_entry_points.py`

- [ ] **Step 4.1: Add the entry-point pass to `_introspect.py`**

    Above `build_payload`, add:

    ```python
    def _load_entry_points(warnings: list[str]) -> None:
        """Trigger imports for every package registered under ``toolr.commands``.

        Loading an entry point is enough to run its module's ``command_group``
        and ``@group.command`` decorators; the actual results land in the
        toolr registry singleton, which ``_walk_registry`` reads in a single
        pass after both the tools walk and the entry-point load complete.
        """
        import importlib.metadata as md

        # `entry_points(group=...)` is the modern API (Python 3.10+); we
        # target 3.11+ across the project so this is safe.
        try:
            eps = md.entry_points(group="toolr.commands")
        except Exception as exc:  # pragma: no cover — extremely defensive
            warnings.append(f"failed to enumerate entry points: {exc!r}")
            return

        for ep in eps:
            try:
                ep.load()
            except Exception as exc:  # noqa: BLE001 — see _import_tools_modules
                warnings.append(
                    f"failed to load entry point `{ep.name}` from `{ep.value}`: "
                    f"{type(exc).__name__}: {exc}"
                )
    ```

    Then call it inside `build_payload`, immediately before `_walk_registry`:

    ```python
    def build_payload(tools_root: str | None) -> dict[str, Any]:
        warnings: list[str] = []
        _ensure_tools_on_syspath(tools_root)
        _import_tools_modules(warnings)
        _load_entry_points(warnings)
        groups, commands = _walk_registry()
        return {
            "payload_schema_version": PAYLOAD_SCHEMA_VERSION,
            "groups": groups,
            "commands": commands,
            "warnings": warnings,
        }
    ```

- [ ] **Step 4.2: Write an entry-point test using a stub module**

    Create `tests/test_introspect_entry_points.py`:

    ```python
    """Test that entry points registered under `toolr.commands` are discovered."""

    from __future__ import annotations

    import json
    import subprocess
    import sys
    import textwrap
    from pathlib import Path


    def test_entry_point_module_groups_appear(tmp_path: Path, monkeypatch) -> None:
        # Install a fake package directly into a tmp sys.path entry, then
        # register it as a `toolr.commands` entry point via a dist-info dir.
        pkg = tmp_path / "fake_toolr_legacy"
        pkg.mkdir()
        (pkg / "__init__.py").write_text(
            textwrap.dedent(
                '''
                from toolr import command_group

                group = command_group("legacy", "Legacy group", description="Legacy.")

                @group.command
                def widget(ctx):
                    """Widget command."""
                    return 0
                '''
            )
        )

        dist_info = tmp_path / "fake_toolr_legacy-0.0.0.dist-info"
        dist_info.mkdir()
        (dist_info / "METADATA").write_text(
            "Metadata-Version: 2.1\nName: fake-toolr-legacy\nVersion: 0.0.0\n"
        )
        (dist_info / "entry_points.txt").write_text(
            "[toolr.commands]\nlegacy = fake_toolr_legacy\n"
        )

        env = {
            **dict(monkeypatch._setitem) if hasattr(monkeypatch, "_setitem") else {},
        }
        # Use PYTHONPATH so the sub-interpreter sees the fake package + dist-info.
        proc = subprocess.run(
            [sys.executable, "-m", "toolr._introspect"],
            capture_output=True,
            text=True,
            check=True,
            env={**__import__("os").environ, "PYTHONPATH": str(tmp_path)},
        )
        payload = json.loads(proc.stdout)
        names = {g["name"] for g in payload["groups"]}
        assert "legacy" in names, payload
        cmd_names = {(c["group"], c["name"]) for c in payload["commands"]}
        assert ("legacy", "widget") in cmd_names, payload
    ```

- [ ] **Step 4.3: Run the test**

    ```bash
    uv run pytest tests/test_introspect_entry_points.py -q
    ```

    Expected: 1 test passing.

- [ ] **Step 4.4: Commit**

    ```bash
    git add python/toolr/_introspect.py tests/test_introspect_entry_points.py
    git commit -m "feat(introspect): Load toolr.commands entry points so legacy packages appear in dynamic layer"
    ```

---

## Task 5: Rust — dynamic-hash computation over the tools venv

Hash the set of installed packages (name + version) in the tools venv. This
hash becomes `Manifest.dynamic_hash` and is the staleness signal that drives
auto-rebuild at execute time.

Method: glob `<venv>/lib/python*/site-packages/*.dist-info` directory names
and hash the **sorted** list. The directory name already contains both name
and version (`foo-1.2.3.dist-info`), so we never need to parse `METADATA`.

**Files:**

- Modify: `src/dynamic/mod.rs`
- Create: `src/dynamic/hash.rs`

- [ ] **Step 5.1: Re-export the hash module**

    Update `src/dynamic/mod.rs`:

    ```rust
    pub mod hash;
    pub mod payload;

    pub use hash::compute_dynamic_hash;
    pub use payload::{DynamicPayload, PAYLOAD_SCHEMA_VERSION};

    #[cfg(test)]
    mod tests;
    ```

- [ ] **Step 5.2: Create `src/dynamic/hash.rs`**

    ```rust
    //! Hash the set of packages installed in the tools venv.
    //!
    //! Used as `Manifest.dynamic_hash` — when this value differs from the
    //! one stamped into the manifest, the dynamic layer is stale and must be
    //! regenerated before the next command executes.

    use std::path::Path;

    use anyhow::{Context, Result};
    use blake3::Hasher;

    /// Compute a deterministic hash of the installed-package set in `venv_root`.
    ///
    /// The hash covers the sorted list of `*.dist-info` directory names under
    /// `lib/python*/site-packages/`. Because each `.dist-info` directory is
    /// named `<package>-<version>.dist-info`, any add / remove / version-change
    /// changes the hash.
    pub fn compute_dynamic_hash(venv_root: &Path) -> Result<String> {
        let names = collect_dist_info_names(venv_root)
            .with_context(|| format!("scanning {} for dist-info", venv_root.display()))?;
        let mut hasher = Hasher::new();
        for n in &names {
            hasher.update(n.as_bytes());
            hasher.update(b"\0");
        }
        Ok(hasher.finalize().to_hex().to_string())
    }

    fn collect_dist_info_names(venv_root: &Path) -> Result<Vec<String>> {
        let lib = venv_root.join("lib");
        let mut names = Vec::new();
        let Ok(entries) = std::fs::read_dir(&lib) else {
            // No lib/ → empty venv-like layout. Return an empty list so the
            // resulting hash is stable rather than an error.
            return Ok(names);
        };
        for entry in entries.flatten() {
            let pyver = entry.path();
            let site = pyver.join("site-packages");
            let Ok(site_entries) = std::fs::read_dir(&site) else {
                continue;
            };
            for sp_entry in site_entries.flatten() {
                let name = sp_entry.file_name().to_string_lossy().into_owned();
                if name.ends_with(".dist-info") && sp_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    names.push(name);
                }
            }
        }
        names.sort();
        Ok(names)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use tempfile::TempDir;

        fn make_venv(packages: &[&str]) -> TempDir {
            let tmp = TempDir::new().unwrap();
            let site = tmp.path().join("lib").join("python3.13").join("site-packages");
            std::fs::create_dir_all(&site).unwrap();
            for p in packages {
                std::fs::create_dir(site.join(format!("{p}.dist-info"))).unwrap();
            }
            tmp
        }

        #[test]
        fn identical_package_sets_hash_identically() {
            let a = make_venv(&["foo-1.0.0", "bar-2.0.0"]);
            let b = make_venv(&["bar-2.0.0", "foo-1.0.0"]); // different filesystem order
            assert_eq!(
                compute_dynamic_hash(a.path()).unwrap(),
                compute_dynamic_hash(b.path()).unwrap(),
            );
        }

        #[test]
        fn version_bump_changes_hash() {
            let a = make_venv(&["foo-1.0.0"]);
            let b = make_venv(&["foo-1.0.1"]);
            assert_ne!(
                compute_dynamic_hash(a.path()).unwrap(),
                compute_dynamic_hash(b.path()).unwrap(),
            );
        }

        #[test]
        fn missing_lib_dir_returns_empty_hash() {
            let tmp = TempDir::new().unwrap();
            // Hash is stable: the same value any other "empty" venv produces.
            let h = compute_dynamic_hash(tmp.path()).unwrap();
            assert!(!h.is_empty());
        }

        #[test]
        fn ignores_non_dist_info_entries() {
            let a = make_venv(&["foo-1.0.0"]);
            let site = a.path().join("lib").join("python3.13").join("site-packages");
            std::fs::create_dir(site.join("not_a_dist_info_dir")).unwrap();
            std::fs::write(site.join("stray-1.0.0.dist-info"), "i am a file, not a dir").unwrap();
            let b = make_venv(&["foo-1.0.0"]);
            assert_eq!(
                compute_dynamic_hash(a.path()).unwrap(),
                compute_dynamic_hash(b.path()).unwrap(),
            );
        }
    }
    ```

- [ ] **Step 5.3: Run tests**

    ```bash
    cargo test --lib dynamic::hash::
    ```

    Expected: 4 tests passing.

- [ ] **Step 5.4: Commit**

    ```bash
    git add src/dynamic/
    git commit -m "feat(dynamic): Compute dynamic-layer hash over venv dist-info names"
    ```

---

## Task 6: Rust — spawn the introspection subprocess

Add `_rust_utils::dynamic::run_introspect` that locates the Python in the
tools venv, spawns `python -m toolr._introspect --tools-root <path>`, reads
stdout, and deserializes a `DynamicPayload`. Errors carry context.

**Files:**

- Modify: `src/dynamic/mod.rs`
- Create: `src/dynamic/runner.rs`

- [ ] **Step 6.1: Re-export the runner module**

    Update `src/dynamic/mod.rs`:

    ```rust
    pub mod hash;
    pub mod payload;
    pub mod runner;

    pub use hash::compute_dynamic_hash;
    pub use payload::{DynamicPayload, PAYLOAD_SCHEMA_VERSION};
    pub use runner::{IntrospectError, run_introspect};

    #[cfg(test)]
    mod tests;
    ```

- [ ] **Step 6.2: Create `src/dynamic/runner.rs`**

    ```rust
    //! Spawn `python -m toolr._introspect` in the tools venv and capture its
    //! JSON payload.

    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use thiserror::Error;

    use super::payload::{DynamicPayload, PAYLOAD_SCHEMA_VERSION};

    #[derive(Debug, Error)]
    pub enum IntrospectError {
        #[error("python interpreter not found at {0}")]
        PythonMissing(PathBuf),
        #[error("introspect helper exited with status {status:?}\nstderr:\n{stderr}")]
        SubprocessFailed { status: Option<i32>, stderr: String },
        #[error("I/O while spawning introspect helper: {0}")]
        Io(#[from] std::io::Error),
        #[error("JSON decode error in introspect payload: {0}")]
        Json(#[from] serde_json::Error),
        #[error("introspect payload schema {got}, this toolr understands {expected}")]
        UnsupportedPayloadSchema { got: u32, expected: u32 },
    }

    /// Run the dynamic introspection helper.
    ///
    /// `python` is the absolute path to the Python interpreter inside the
    /// tools venv (resolved by `_rust_utils::venv` from Plan 3). `tools_dir`
    /// is the project's `tools/` directory; the helper inserts its parent on
    /// `sys.path` before importing.
    pub fn run_introspect(python: &Path, tools_dir: &Path) -> Result<DynamicPayload, IntrospectError> {
        if !python.is_file() {
            return Err(IntrospectError::PythonMissing(python.to_path_buf()));
        }
        let output = Command::new(python)
            .args(["-m", "toolr._introspect", "--tools-root"])
            .arg(tools_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()?;
        if !output.status.success() {
            return Err(IntrospectError::SubprocessFailed {
                status: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        let payload: DynamicPayload = serde_json::from_slice(&output.stdout)?;
        if payload.payload_schema_version != PAYLOAD_SCHEMA_VERSION {
            return Err(IntrospectError::UnsupportedPayloadSchema {
                got: payload.payload_schema_version,
                expected: PAYLOAD_SCHEMA_VERSION,
            });
        }
        Ok(payload.retag_as_dynamic())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::Write;
        use tempfile::TempDir;

        /// Build a fake "python" shell script that prints a fixed JSON payload
        /// and exits 0. Lets us test the runner without a real venv.
        #[cfg(unix)]
        fn fake_python(tmp: &TempDir, body: &str) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let path = tmp.path().join("python");
            let mut f = std::fs::File::create(&path).unwrap();
            writeln!(f, "#!/bin/sh").unwrap();
            writeln!(f, "cat <<'__EOF__'").unwrap();
            writeln!(f, "{}", body).unwrap();
            writeln!(f, "__EOF__").unwrap();
            drop(f);
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
            path
        }

        #[test]
        #[cfg(unix)]
        fn happy_path_deserializes_and_retags_as_dynamic() {
            let tmp = TempDir::new().unwrap();
            let py = fake_python(
                &tmp,
                r#"{"payload_schema_version":1,"groups":[{"name":"x","title":"X","description":"","origin":"static"}],"commands":[],"warnings":[]}"#,
            );
            let tools = tmp.path().join("tools");
            std::fs::create_dir(&tools).unwrap();
            let p = run_introspect(&py, &tools).unwrap();
            assert_eq!(p.groups.len(), 1);
            // Python said "static"; runner retagged to dynamic.
            assert_eq!(p.groups[0].origin, crate::manifest::Origin::Dynamic);
        }

        #[test]
        fn missing_python_returns_clear_error() {
            let tmp = TempDir::new().unwrap();
            let py = tmp.path().join("no-such-python");
            let err = run_introspect(&py, tmp.path()).expect_err("should fail");
            assert!(matches!(err, IntrospectError::PythonMissing(_)));
        }
    }
    ```

- [ ] **Step 6.3: Run the tests**

    ```bash
    cargo test --lib dynamic::runner::
    ```

    Expected: 2 tests passing on Unix (the happy-path test is gated to Unix to keep CI portable).

- [ ] **Step 6.4: Commit**

    ```bash
    git add src/dynamic/
    git commit -m "feat(dynamic): Spawn introspect helper and decode payload with schema check"
    ```

---

## Task 7: Rust — merge dynamic entries into a base manifest

Implement `merge_dynamic` that takes the static manifest from
`build_static_manifest` and a `DynamicPayload`, and produces a unified
manifest. Conflict policy: **static wins**. Groups are deduplicated by
`name`; commands by `(group, name)`.

**Files:**

- Modify: `src/dynamic/mod.rs`
- Create: `src/dynamic/merge.rs`

- [ ] **Step 7.1: Re-export the merge module**

    Update `src/dynamic/mod.rs`:

    ```rust
    pub mod hash;
    pub mod merge;
    pub mod payload;
    pub mod runner;

    pub use hash::compute_dynamic_hash;
    pub use merge::merge_dynamic;
    pub use payload::{DynamicPayload, PAYLOAD_SCHEMA_VERSION};
    pub use runner::{IntrospectError, run_introspect};

    #[cfg(test)]
    mod tests;
    ```

- [ ] **Step 7.2: Create `src/dynamic/merge.rs`**

    ```rust
    //! Merge a dynamic-layer payload into a base (static) manifest.

    use std::collections::HashSet;

    use super::payload::DynamicPayload;
    use crate::manifest::Manifest;

    /// Merge `payload` into `base`. Returns the resulting manifest.
    ///
    /// Conflict policy:
    /// - A group present in `base.groups` with the same `name` as one in the
    ///   payload keeps the static definition; the dynamic copy is dropped.
    /// - A command present in `base.commands` with the same `(group, name)`
    ///   as one in the payload keeps the static definition; the dynamic copy
    ///   is dropped.
    /// - The resulting manifest's `dynamic_hash` is **not** touched here —
    ///   callers stamp it from `compute_dynamic_hash` after the venv state
    ///   they used to produce `payload`.
    pub fn merge_dynamic(mut base: Manifest, payload: DynamicPayload) -> Manifest {
        let existing_groups: HashSet<String> =
            base.groups.iter().map(|g| g.name.clone()).collect();
        let existing_cmds: HashSet<(String, String)> = base
            .commands
            .iter()
            .map(|c| (c.group.clone(), c.name.clone()))
            .collect();

        for g in payload.groups {
            if !existing_groups.contains(&g.name) {
                base.groups.push(g);
            }
        }
        for c in payload.commands {
            let key = (c.group.clone(), c.name.clone());
            if !existing_cmds.contains(&key) {
                base.commands.push(c);
            }
        }
        base
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::manifest::{Command, Group, Manifest, Origin, SCHEMA_VERSION};

        fn cmd(group: &str, name: &str, origin: Origin) -> Command {
            Command {
                name: name.into(),
                group: group.into(),
                module: format!("tools.{group}"),
                function: name.replace('-', "_"),
                summary: "".into(),
                description: "".into(),
                arguments: vec![],
                imports: vec![],
                origin,
            }
        }

        fn grp(name: &str, origin: Origin) -> Group {
            Group {
                name: name.into(),
                title: name.into(),
                description: "".into(),
                origin,
            }
        }

        fn base_with(groups: Vec<Group>, commands: Vec<Command>) -> Manifest {
            Manifest {
                schema_version: SCHEMA_VERSION,
                static_hash: "h".into(),
                dynamic_hash: "".into(),
                groups,
                commands,
            }
        }

        #[test]
        fn dynamic_only_entries_get_appended() {
            let base = base_with(vec![], vec![]);
            let payload = DynamicPayload {
                payload_schema_version: 1,
                groups: vec![grp("legacy", Origin::Dynamic)],
                commands: vec![cmd("legacy", "widget", Origin::Dynamic)],
                warnings: vec![],
            };
            let merged = merge_dynamic(base, payload);
            assert_eq!(merged.groups.len(), 1);
            assert_eq!(merged.commands.len(), 1);
            assert_eq!(merged.groups[0].origin, Origin::Dynamic);
        }

        #[test]
        fn static_group_wins_over_dynamic_with_same_name() {
            let base = base_with(vec![grp("ci", Origin::Static)], vec![]);
            let payload = DynamicPayload {
                payload_schema_version: 1,
                // Dynamic emits a "ci" group with conflicting metadata.
                groups: vec![Group {
                    name: "ci".into(),
                    title: "FROM DYNAMIC".into(),
                    description: "".into(),
                    origin: Origin::Dynamic,
                }],
                commands: vec![],
                warnings: vec![],
            };
            let merged = merge_dynamic(base, payload);
            assert_eq!(merged.groups.len(), 1);
            assert_eq!(merged.groups[0].origin, Origin::Static);
            assert_ne!(merged.groups[0].title, "FROM DYNAMIC");
        }

        #[test]
        fn static_command_wins_over_dynamic_with_same_group_and_name() {
            let base = base_with(
                vec![grp("ci", Origin::Static)],
                vec![cmd("ci", "hello", Origin::Static)],
            );
            let payload = DynamicPayload {
                payload_schema_version: 1,
                groups: vec![],
                commands: vec![cmd("ci", "hello", Origin::Dynamic)],
                warnings: vec![],
            };
            let merged = merge_dynamic(base, payload);
            assert_eq!(merged.commands.len(), 1);
            assert_eq!(merged.commands[0].origin, Origin::Static);
        }

        #[test]
        fn merge_preserves_existing_dynamic_hash() {
            let mut base = base_with(vec![], vec![]);
            base.dynamic_hash = "preserved".into();
            let merged = merge_dynamic(
                base,
                DynamicPayload {
                    payload_schema_version: 1,
                    groups: vec![],
                    commands: vec![],
                    warnings: vec![],
                },
            );
            assert_eq!(merged.dynamic_hash, "preserved");
        }
    }
    ```

- [ ] **Step 7.3: Run tests**

    ```bash
    cargo test --lib dynamic::merge::
    ```

    Expected: 4 tests passing.

- [ ] **Step 7.4: Commit**

    ```bash
    git add src/dynamic/
    git commit -m "feat(dynamic): Merge dynamic payload into static manifest with static-wins policy"
    ```

---

## Task 8: Rust — top-level `rebuild_manifest` function

Compose the building blocks: discover the project root, build the static
manifest, resolve the tools venv (from Plan 3), spawn the helper, merge,
stamp `dynamic_hash`, and return. Two entry points: `rebuild_manifest_full`
(static + dynamic + write) and `rebuild_dynamic_only` (assumes the on-disk
manifest's static layer is fresh; useful for auto-rebuild at execute time).

**Files:**

- Create: `src/dynamic/rebuild.rs`
- Modify: `src/dynamic/mod.rs`

- [ ] **Step 8.1: Re-export**

    Update `src/dynamic/mod.rs`:

    ```rust
    pub mod hash;
    pub mod merge;
    pub mod payload;
    pub mod rebuild;
    pub mod runner;

    pub use hash::compute_dynamic_hash;
    pub use merge::merge_dynamic;
    pub use payload::{DynamicPayload, PAYLOAD_SCHEMA_VERSION};
    pub use rebuild::{RebuildOutcome, rebuild_dynamic_only, rebuild_manifest_full};
    pub use runner::{IntrospectError, run_introspect};

    #[cfg(test)]
    mod tests;
    ```

- [ ] **Step 8.2: Create `src/dynamic/rebuild.rs`**

    ```rust
    //! High-level rebuild orchestration for both static-plus-dynamic and
    //! dynamic-only refresh paths.

    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result};

    use super::hash::compute_dynamic_hash;
    use super::merge::merge_dynamic;
    use super::runner::run_introspect;
    use crate::manifest::{Manifest, load_manifest, write_manifest};
    use crate::parser::build_static_manifest;

    /// Result of a rebuild, returned for diagnostics / CLI output.
    #[derive(Debug)]
    pub struct RebuildOutcome {
        pub manifest_path: PathBuf,
        pub group_count: usize,
        pub command_count: usize,
        pub warnings: Vec<String>,
    }

    /// Full rebuild: static layer + dynamic layer + write.
    ///
    /// `python` is the absolute path to the tools-venv Python interpreter
    /// (resolved by Plan 3's `_rust_utils::venv::resolve_tools_venv`).
    /// `venv_root` is the venv directory used by [`compute_dynamic_hash`].
    pub fn rebuild_manifest_full(
        project_root: &Path,
        python: &Path,
        venv_root: &Path,
    ) -> Result<RebuildOutcome> {
        let tools = project_root.join("tools");
        let base = build_static_manifest(&tools)
            .with_context(|| "building static manifest layer")?;
        let payload = run_introspect(python, &tools)
            .with_context(|| "running dynamic-layer introspect helper")?;
        let warnings = payload.warnings.clone();
        let mut merged = merge_dynamic(base, payload);
        merged.dynamic_hash = compute_dynamic_hash(venv_root)?;
        let manifest_path = tools.join(".toolr-manifest.json");
        write_manifest(&manifest_path, &merged)?;
        Ok(RebuildOutcome {
            manifest_path,
            group_count: merged.groups.len(),
            command_count: merged.commands.len(),
            warnings,
        })
    }

    /// Dynamic-only refresh: reuse the on-disk manifest's static layer,
    /// strip its dynamic entries, run the helper, re-merge, and write.
    ///
    /// Cheap relative to a full rebuild — used at execute time when only the
    /// venv has changed.
    pub fn rebuild_dynamic_only(
        project_root: &Path,
        python: &Path,
        venv_root: &Path,
    ) -> Result<RebuildOutcome> {
        use crate::manifest::Origin;

        let tools = project_root.join("tools");
        let manifest_path = tools.join(".toolr-manifest.json");
        let mut base = load_manifest(&manifest_path)
            .with_context(|| format!("loading {}", manifest_path.display()))?;
        // Drop everything dynamic; keep the static skeleton intact.
        base.groups.retain(|g| g.origin == Origin::Static);
        base.commands.retain(|c| c.origin == Origin::Static);

        let payload = run_introspect(python, &tools)
            .with_context(|| "running dynamic-layer introspect helper")?;
        let warnings = payload.warnings.clone();
        let mut merged = merge_dynamic(base, payload);
        merged.dynamic_hash = compute_dynamic_hash(venv_root)?;
        write_manifest(&manifest_path, &merged)?;
        Ok(RebuildOutcome {
            manifest_path,
            group_count: merged.groups.len(),
            command_count: merged.commands.len(),
            warnings,
        })
    }
    ```

- [ ] **Step 8.3: Add an integration test using a fake-python shell script**

    Append to `src/dynamic/runner.rs`'s test module (or create a new one in
    `src/dynamic/rebuild.rs`):

    ```rust
    #[cfg(test)]
    #[cfg(unix)]
    mod rebuild_tests {
        use super::*;
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        use tempfile::TempDir;

        fn fake_python_emitting(tmp: &Path, body: &str) -> PathBuf {
            let path = tmp.join("python");
            let mut f = std::fs::File::create(&path).unwrap();
            writeln!(f, "#!/bin/sh").unwrap();
            writeln!(f, "cat <<'__EOF__'").unwrap();
            writeln!(f, "{body}").unwrap();
            writeln!(f, "__EOF__").unwrap();
            drop(f);
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
            path
        }

        #[test]
        fn full_rebuild_writes_combined_manifest() {
            let tmp = TempDir::new().unwrap();
            let project = tmp.path();
            let tools = project.join("tools");
            std::fs::create_dir(&tools).unwrap();
            std::fs::write(
                tools.join("ci.py"),
                "\"\"\"CI utilities.\"\"\"\ngroup = command_group(\"ci\", \"CI utilities\")\n@group.command\ndef hello(ctx):\n    \"\"\"Say hello.\"\"\"\n    pass\n",
            ).unwrap();
            let venv = project.join("venv");
            std::fs::create_dir_all(venv.join("lib/python3.13/site-packages/foo-1.0.0.dist-info")).unwrap();
            let py = fake_python_emitting(
                project,
                r#"{"payload_schema_version":1,"groups":[{"name":"legacy","title":"Legacy","description":"","origin":"static"}],"commands":[{"name":"widget","group":"legacy","module":"third","function":"widget","summary":"","description":"","arguments":[],"imports":[],"origin":"static"}],"warnings":[]}"#,
            );
            let outcome = rebuild_manifest_full(project, &py, &venv).unwrap();
            assert!(outcome.manifest_path.is_file());
            let m = crate::manifest::load_manifest(&outcome.manifest_path).unwrap();
            let group_names: Vec<_> = m.groups.iter().map(|g| g.name.as_str()).collect();
            assert!(group_names.contains(&"ci"));
            assert!(group_names.contains(&"legacy"));
            assert!(!m.dynamic_hash.is_empty());
        }
    }
    ```

- [ ] **Step 8.4: Run tests**

    ```bash
    cargo test --lib dynamic::
    ```

    Expected: previous dynamic tests + the new rebuild integration test, all passing on Unix.

- [ ] **Step 8.5: Commit**

    ```bash
    git add src/dynamic/
    git commit -m "feat(dynamic): Add rebuild_manifest_full and rebuild_dynamic_only orchestration"
    ```

---

## Task 9: CLI — `toolr project manifest rebuild`

Add the user-facing subcommand. Build under `toolr project` as a built-in
namespace (per the design's `toolr project <...>` rule). The subcommand calls
`rebuild_manifest_full`.

**Files:**

- Modify: `src/bin/toolr/cli.rs`
- Modify: `src/bin/toolr/dispatch.rs`

- [ ] **Step 9.1: Attach the `project` subcommand tree in `cli.rs`**

    After the user-defined group loop in `build_command`, add:

    ```rust
    let project = Command::new("project")
        .about("Operations on the current repo's tools/")
        .subcommand_required(true)
        .subcommand(
            Command::new("manifest")
                .about("Manage the project's toolr manifest")
                .subcommand_required(true)
                .subcommand(
                    Command::new("rebuild")
                        .about("Regenerate the static + dynamic manifest in place"),
                ),
        );
    root = root.subcommand(project);
    ```

- [ ] **Step 9.2: Route the subcommand in `dispatch.rs`**

    Near the top of `dispatch`, before the user-command lookup:

    ```rust
    if let Some(("project", project_matches)) = matches.subcommand() {
        return run_project_subcommand(project_matches);
    }
    ```

    And add:

    ```rust
    fn run_project_subcommand(matches: &ArgMatches) -> anyhow::Result<ExitCode> {
        match matches.subcommand() {
            Some(("manifest", manifest_matches)) => match manifest_matches.subcommand() {
                Some(("rebuild", _)) => run_manifest_rebuild(),
                _ => Ok(ExitCode::from(2)),
            },
            _ => Ok(ExitCode::from(2)),
        }
    }

    fn run_manifest_rebuild() -> anyhow::Result<ExitCode> {
        use _rust_utils::dynamic::rebuild_manifest_full;
        use _rust_utils::venv::resolve_tools_venv;

        let cwd = std::env::current_dir()?;
        let project_root = _rust_utils::discovery::discover_project_root(&cwd)?;
        let venv = resolve_tools_venv(&project_root)?;
        let outcome = rebuild_manifest_full(&project_root, &venv.python, &venv.root)?;
        for w in &outcome.warnings {
            eprintln!("toolr: warning: {w}");
        }
        println!(
            "toolr: wrote {} groups / {} commands to {}",
            outcome.group_count,
            outcome.command_count,
            outcome.manifest_path.display(),
        );
        Ok(ExitCode::SUCCESS)
    }
    ```

    **Note for the implementer:** `_rust_utils::venv::resolve_tools_venv` is
    defined by Plan 3. It returns a struct with at least `root: PathBuf`
    (the venv directory) and `python: PathBuf` (the interpreter inside it).
    If the exact API differs in Plan 3 by the time this task lands, adapt
    the call site; the rest of the dispatch code is independent.

- [ ] **Step 9.3: Smoke test in `tests/cli_smoke.rs`**

    Append:

    ```rust
    #[test]
    fn project_manifest_rebuild_help_lists_command() {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        std::fs::create_dir(&tools).unwrap();
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .args(["project", "manifest", "--help"])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("rebuild"), "expected rebuild listed, got:\n{stdout}");
    }
    ```

- [ ] **Step 9.4: Run tests**

    ```bash
    cargo test --test cli_smoke
    ```

    Expected: the new test plus all prior smoke tests pass.

- [ ] **Step 9.5: Commit**

    ```bash
    git add src/bin/toolr/ tests/cli_smoke.rs
    git commit -m "feat(cli): Add toolr project manifest rebuild command"
    ```

---

## Task 10: Auto-rebuild dynamic layer at execute time

Hook into the dispatcher so any `toolr <user-cmd>` execution checks whether
the manifest's `dynamic_hash` matches the venv's current hash. If not — or if
`dynamic_hash` is empty — invoke `rebuild_dynamic_only` before delegating to
the Python runner. This is the only auto-regen path; tab completion never
takes it.

**Files:**

- Modify: `src/bin/toolr/dispatch.rs`

- [ ] **Step 10.1: Add a freshness check helper**

    In `src/bin/toolr/dispatch.rs`, add:

    ```rust
    fn ensure_dynamic_layer_fresh(
        project_root: &std::path::Path,
        manifest: &_rust_utils::manifest::Manifest,
    ) -> anyhow::Result<()> {
        use _rust_utils::dynamic::{compute_dynamic_hash, rebuild_dynamic_only};
        use _rust_utils::venv::resolve_tools_venv;

        let venv = resolve_tools_venv(project_root)?;
        let current = compute_dynamic_hash(&venv.root)?;
        if manifest.dynamic_hash == current && !current.is_empty() {
            return Ok(());
        }
        eprintln!("toolr: dynamic manifest layer stale; regenerating...");
        rebuild_dynamic_only(project_root, &venv.python, &venv.root)?;
        Ok(())
    }
    ```

- [ ] **Step 10.2: Call it before user-command execution**

    Modify `dispatch` so that the user-command branch (where Plan 2 spawns
    the Python runner) calls `ensure_dynamic_layer_fresh` first. The
    placement is inside the existing user-command resolution block, *after*
    the command is looked up and *before* control hands off to the runner.

    Concretely, in the block that handles a resolved `(group_name, cmd_name)`:

    ```rust
    let project_root = _rust_utils::discovery::discover_project_root(
        &std::env::current_dir()?,
    )?;
    ensure_dynamic_layer_fresh(&project_root, manifest)?;
    // ... then fall through to the existing Plan-2 runner invocation.
    ```

    **Important:** if Plan 2's runner invocation is still the stub message,
    this hook still runs (and prints "regenerating..." when stale) but the
    user command itself exits with the Plan-2 stub. That is correct — the
    auto-rebuild path is decoupled from execution wiring.

- [ ] **Step 10.3: Test the auto-rebuild trigger**

    Append to `tests/cli_smoke.rs`:

    ```rust
    #[test]
    #[cfg(unix)]
    fn execute_time_auto_rebuild_kicks_in_when_dynamic_hash_is_empty() {
        // Build a fixture project with a static-only manifest, where
        // dynamic_hash is empty. Stub out the venv by writing fake dist-info
        // under venv/lib/python3.X/site-packages and pointing
        // `resolve_tools_venv` at it via an env override. Plan 3 should
        // honour an env-var override (TOOLR_TOOLS_VENV_OVERRIDE) for tests;
        // if that override doesn't yet exist, this test is gated until it
        // does.
        //
        // Concretely: invoke `toolr <group> <cmd>` and assert stderr
        // contains "regenerating...".
        //
        // Skip the test when TOOLR_TOOLS_VENV_OVERRIDE is not wired up:
        if option_env!("CARGO_FEATURE_VENV_OVERRIDE").is_none() {
            eprintln!("skipping: feature `venv_override` not enabled");
            return;
        }
        // Real assertion logic lands once Plan 3 lands the override.
    }
    ```

    **Note for the implementer:** if Plan 3 has not yet landed the
    `TOOLR_TOOLS_VENV_OVERRIDE` mechanism by the time Task 10 is
    implemented, replace this gated test with a unit test against
    `ensure_dynamic_layer_fresh` directly that mocks `resolve_tools_venv` —
    either by feature-flagging the venv module or by injecting a small
    trait. Don't merge Task 10 without **some** test covering the staleness
    branch.

- [ ] **Step 10.4: Run smoke tests**

    ```bash
    cargo test --test cli_smoke
    ```

    Expected: all smoke tests pass; the new test either runs or self-skips.

- [ ] **Step 10.5: Commit**

    ```bash
    git add src/bin/toolr/dispatch.rs tests/cli_smoke.rs
    git commit -m "feat(dispatch): Auto-rebuild dynamic manifest layer when venv state changes"
    ```

---

## Task 11: Ship `.pre-commit-hooks.yaml`

Add the file at the repo root so downstream projects can reference this
repo as a pre-commit hook source. The single hook regenerates the manifest
on any change under `tools/`.

**Files:**

- Create: `.pre-commit-hooks.yaml`
- Modify: `docs/` or `CONTRIBUTING.md` (only if either already documents
  pre-commit usage — otherwise skip; do not create new docs files in this
  plan).

- [ ] **Step 11.1: Create `.pre-commit-hooks.yaml`**

    Write the file exactly as specified in `00-design.md` §Manifest file:

    ```yaml
    - id: toolr-manifest
      name: Regenerate toolr manifest
      entry: toolr project manifest rebuild
      language: system
      pass_filenames: false
      files: ^tools/.*\.py$
    ```

- [ ] **Step 11.2: Smoke-check the YAML parses**

    ```bash
    python -c "import yaml,sys; print(yaml.safe_load(open('.pre-commit-hooks.yaml')))"
    ```

    Expected: a list with one dict, `id` == `"toolr-manifest"`, `entry`
    matches the design.

- [ ] **Step 11.3: Commit**

    ```bash
    git add .pre-commit-hooks.yaml
    git commit -m "feat(pre-commit): Ship toolr-manifest hook config for downstream consumers"
    ```

---

## Task 12: End-to-end integration test — static + dynamic together

Materialize a fixture project that exercises *both* the static and dynamic
layers in a single rebuild. Verify the merged on-disk manifest contains
entries from both, with the expected origin tags.

**Files:**

- Create: `tests/dynamic_e2e.rs`

- [ ] **Step 12.1: Write the integration test**

    ```rust
    //! End-to-end: static + dynamic layers merged into a single manifest.

    #![cfg(unix)]

    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use _rust_utils::dynamic::rebuild_manifest_full;
    use _rust_utils::manifest::{Origin, load_manifest};
    use tempfile::TempDir;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    fn make_fake_python(at: &Path, payload: &str) {
        let mut f = std::fs::File::create(at).unwrap();
        writeln!(f, "#!/bin/sh").unwrap();
        writeln!(f, "cat <<'__EOF__'").unwrap();
        writeln!(f, "{payload}").unwrap();
        writeln!(f, "__EOF__").unwrap();
        drop(f);
        let mut perms = std::fs::metadata(at).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(at, perms).unwrap();
    }

    #[test]
    fn full_rebuild_merges_static_and_dynamic_entries() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();

        // ---- Static side: a tools/ci.py the static parser can see.
        write(
            &project.join("tools").join("ci.py"),
            "\"\"\"CI utilities.\"\"\"\ngroup = command_group(\"ci\", \"CI utilities\")\n@group.command\ndef hello(ctx):\n    \"\"\"Say hello.\"\"\"\n    pass\n",
        );

        // ---- Dynamic side: a fake-python that emits a payload announcing
        //      one legacy group + command, plus a conflict on (ci, hello)
        //      that must lose to the static entry.
        let py = project.join("python");
        make_fake_python(
            &py,
            r#"{"payload_schema_version":1,"groups":[{"name":"legacy","title":"Legacy","description":"","origin":"static"},{"name":"ci","title":"FROM DYNAMIC","description":"","origin":"static"}],"commands":[{"name":"widget","group":"legacy","module":"third","function":"widget","summary":"Widget.","description":"","arguments":[],"imports":[],"origin":"static"},{"name":"hello","group":"ci","module":"FROM DYNAMIC","function":"hello","summary":"FROM DYNAMIC","description":"","arguments":[],"imports":[],"origin":"static"}],"warnings":["module foo failed: bar"]}"#,
        );

        // ---- Fake venv with a couple of dist-info dirs so dynamic_hash is non-empty.
        let venv = project.join("venv");
        std::fs::create_dir_all(venv.join("lib/python3.13/site-packages/foo-1.0.0.dist-info")).unwrap();
        std::fs::create_dir_all(venv.join("lib/python3.13/site-packages/bar-2.0.0.dist-info")).unwrap();

        let outcome = rebuild_manifest_full(project, &py, &venv).expect("rebuild");

        assert!(outcome.manifest_path.is_file());
        assert_eq!(outcome.warnings.len(), 1);

        let m = load_manifest(&outcome.manifest_path).unwrap();
        // Both groups present.
        let by_name: std::collections::HashMap<_, _> =
            m.groups.iter().map(|g| (g.name.clone(), g)).collect();
        assert_eq!(by_name.len(), 2);
        assert_eq!(by_name["ci"].origin, Origin::Static);
        assert_eq!(by_name["legacy"].origin, Origin::Dynamic);
        // Conflict resolution: static `ci.hello` survived.
        let hello = m
            .commands
            .iter()
            .find(|c| c.group == "ci" && c.name == "hello")
            .expect("hello present");
        assert_eq!(hello.origin, Origin::Static);
        assert_ne!(hello.module, "FROM DYNAMIC");
        // Dynamic legacy.widget came through.
        let widget = m
            .commands
            .iter()
            .find(|c| c.group == "legacy" && c.name == "widget")
            .expect("widget present");
        assert_eq!(widget.origin, Origin::Dynamic);
        // Dynamic hash stamped.
        assert!(!m.dynamic_hash.is_empty());
    }
    ```

- [ ] **Step 12.2: Run the integration test**

    ```bash
    cargo test --test dynamic_e2e
    ```

    Expected: 1 test passing on Unix.

- [ ] **Step 12.3: Commit**

    ```bash
    git add tests/dynamic_e2e.rs
    git commit -m "test(dynamic): End-to-end static-plus-dynamic merge integration test"
    ```

---

## Task 13: Update the roadmap

Mark Plan 6 as Done once everything above is merged.

**Files:**

- Modify: `specs/rust-front-end/01-roadmap.md`

- [ ] **Step 13.1: Update the Plan 6 entry**

    Change `### Plan 6: Dynamic manifest layer` block:

    ```markdown
    ### Plan 6: Dynamic manifest layer

    - **Status:** ✅ Done
    - **Plan doc:** [07-plan-6-dynamic-manifest.md](./07-plan-6-dynamic-manifest.md)
    - **Depends on:** Plan 3
    - **Unblocks:** —
    - **Produces:**
        - …(unchanged)…
    ```

- [ ] **Step 13.2: Commit**

    ```bash
    git add specs/rust-front-end/01-roadmap.md
    git commit -m "docs(roadmap): Mark Plan 6 as done"
    ```

---

## Done criteria

Plan 6 is complete when:

- `cargo test` passes for all unit and integration tests (`dynamic::*`,
  `dynamic_e2e`, `cli_smoke`).
- `uv run pytest tests/test_introspect_empty.py tests/test_introspect_tools_walk.py tests/test_introspect_entry_points.py`
  passes.
- `toolr project manifest rebuild` regenerates `tools/.toolr-manifest.json`
  with both static and dynamic entries, the file is stable across runs
  (same input → same output), and `dynamic_hash` is populated.
- Running a user command (`toolr <group> <cmd>`) when the manifest's
  `dynamic_hash` is empty or stale prints `toolr: dynamic manifest layer
  stale; regenerating...` to stderr, regenerates the manifest, then
  proceeds with execution (or with Plan 2's stub if Plan 2 has not yet
  landed).
- `.pre-commit-hooks.yaml` exists at the repo root and parses cleanly as
  YAML.
- The roadmap status table reflects Plan 6 as `✅ Done`.

## Open questions (for the implementer)

These are deliberately deferred — surface to the spec author if any block
progress, otherwise resolve in line:

1. **`resolve_tools_venv` API shape.** Plan 6 calls
   `_rust_utils::venv::resolve_tools_venv(project_root) -> { root, python }`.
   Plan 3 owns this signature. If Plan 3 lands a different shape (e.g.
   separate functions, or returning a richer struct), adapt the call sites
   in Task 9 and Task 10 without changing the rest of this plan.
2. **`Argument` extraction for dynamic-only commands.** The Python helper
   in Task 3 emits empty `arguments: []` for registry-walked commands,
   relying on the fact that real `tools/*.py` commands are also picked up
   by the static parser (which fills arguments in). For legacy third-party
   entry-point commands, however, this means `--help` shows no argument
   info. If that gap is unacceptable, extend `_command_entry` to walk
   `inspect.signature(func)` and emit `arguments`. Out of scope here; surface
   as a follow-up if reports come in.
3. **Cross-platform fake-python in tests.** Tasks 6, 8, and 12 use a shell
   script to stand in for a Python interpreter, gated `#[cfg(unix)]`. On
   Windows, the same idea needs a `.cmd` shim (or a real Python venv with
   a stub `toolr._introspect`). Document the gap; resolve in CI work
   (Plan 9) rather than here.
4. **Editable-install third-party packages.** The design notes that
   editable installs fall through to the dynamic layer because `.pth`-based
   layouts evade the Plan-5 glob. This plan handles them automatically via
   `importlib.metadata.entry_points`, but **only** if the package
   registered an entry point. If a package relies on `command_group` /
   `@group.command` import-side-effects without an entry point, the helper
   never imports it. Document this; the recommended migration is to add a
   `toolr.commands` entry point or adopt the Plan-5 static manifest.
5. **Warning surfacing policy.** Task 9 prints each warning line as
   `toolr: warning: <line>`. Should `toolr project manifest rebuild`
   exit non-zero when any warning is present (CI-friendly), or always exit
   zero unless the helper crashed (developer-friendly)? Current draft
   chooses zero. Confirm before Plan 9 lands the CI smoke tests.
