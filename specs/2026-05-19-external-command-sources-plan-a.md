# External command sources — Plan A (argparse scanner)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the built-in Rust argparse scanner and the `DispatchCommand` dispatcher contract specified in `specs/2026-05-19-external-command-sources-design.md`. After this plan lands, the user can declare `[tool.toolr.argparse.<name>]` blocks in `tools/pyproject.toml`, point them at `.py` files containing `parser.add_argument(...)` calls, and graft the discovered commands under user-written dispatchers annotated with `dispatched: DispatchCommand`.

**Architecture:** Four pieces.

1. New `toolr.sources` Python module — msgspec types: `ArgSchema`, `CommandSchema`, `DispatchCommand` + `.argv`. Primarily an internal wire format for Rust→Python serialisation; `DispatchCommand` is the only piece end-users touch.
2. Dispatcher detection rule in `toolr.utils._signature` — annotation-driven, scans for any keyword-only parameter annotated `DispatchCommand`.
3. New Rust module `crates/toolr-core/src/argparse/` — reads `[tool.toolr.argparse.*]`, AST-walks scan_paths via `ruff_python_parser`, extracts `add_argument(...)` calls, applies `common_args`, grafts children with `dispatched_from`.
4. Rust-side runtime in `crates/toolr/src/cli.rs` — detects matched leaves with `dispatched_from`, packs the child's parsed kwargs, constructs a `DispatchCommand` Python object, and injects it into the parent function call.

**Tech Stack:** Python (msgspec, inspect), Rust + clap + pyo3 + ruff_python_parser + toml + glob, pytest. Spec: `specs/2026-05-19-external-command-sources-design.md`.

**Read first:** the spec linked above. This plan is the *how*; the spec is the *what* and *why*. Note the schema-version policy: **no `SCHEMA_VERSION` bumps** — toolr is pre-1.0, optional fields are added to v1 in place.

---

## File map

### New Python files (under `crates/toolr-py/python/toolr/sources/`)

- `__init__.py` — public re-exports.
- `_types.py` — `ArgSchema`, `CommandSchema`.
- `_dispatch.py` — `DispatchCommand` + `.argv` reconstruction.

### New test files

- `tests/sources/__init__.py`
- `tests/sources/test_types.py`
- `tests/sources/test_dispatch.py`
- `tests/sources/test_detection.py`
- `tests/sources/test_e2e.py`
- `tests/argparse_scanner/test_scanner.py` — Rust scanner end-to-end via fixtures.

### Modified Python files

- `crates/toolr-py/python/toolr/utils/_signature.py` — dispatcher detection rule.

### New Rust files (under `crates/toolr-core/src/argparse/`)

- `mod.rs` — module entry, public types.
- `config.rs` — parse `[tool.toolr.argparse.*]` from `pyproject.toml`.
- `scan.rs` — AST walk + `add_argument` extraction.
- `attach.rs` — apply `common_args`, graft children under parents, hard-fail matrix.
- `fixtures/` — `.py` golden fixtures + their expected `CommandSchema` JSON.

### Modified Rust files

- `crates/toolr-core/src/manifest/model.rs` — add optional `dispatched_from: Option<String>` to `Command`.
- `crates/toolr-core/src/parser/build.rs` (or a sibling — locate during Task 15) — call into the new `argparse` module from `build_static_manifest`.
- `crates/toolr-core/src/lib.rs` — `pub mod argparse;`.
- `crates/toolr/src/cli.rs` — runtime dispatch detection + child-arg packing.
- `crates/toolr/src/dispatch.rs` (or sibling — locate during Task 17) — pyo3 construction of `DispatchCommand` and parent invocation.

---

## Task index

- Phase 1 — Schemas (1-5)
- Phase 2 — Detection rule (6)
- Phase 3 — `dispatched_from` field (7)
- Phase 4 — Argparse scanner (8-14)
- Phase 5 — Static-manifest integration (15)
- Phase 6 — Runtime injection (16-17)
- Phase 7 — E2E (18-20)

---

## Phase 1 — Schemas

### Task 1: `ArgSchema` + module scaffold

**Files:**

- Create: `crates/toolr-py/python/toolr/sources/__init__.py`
- Create: `crates/toolr-py/python/toolr/sources/_types.py`
- Create: `tests/sources/__init__.py`
- Create: `tests/sources/test_types.py`
- [ ] **Step 1: Write the failing test**

`tests/sources/test_types.py`:

```python
"""Round-trip and field-default tests for toolr.sources schema types."""

from __future__ import annotations

import msgspec

from toolr.sources import ArgSchema


def test_arg_schema_positional_minimal():
    arg = ArgSchema(name="app_label", kind="positional", help="Target app")
    assert arg.name == "app_label"
    assert arg.kind == "positional"
    assert arg.help == "Target app"
    assert arg.default is None
    assert arg.choices is None
    assert arg.metavar is None
    assert arg.type_annotation is None
    assert arg.nargs is None


def test_arg_schema_round_trips_through_msgspec_json():
    arg = ArgSchema(
        name="database",
        kind="optional",
        help="Database to use",
        default="default",
        type_annotation="str",
    )
    payload = msgspec.json.encode(arg)
    decoded = msgspec.json.decode(payload, type=ArgSchema)
    assert decoded == arg
```

- [ ] **Step 2: Run test to verify it fails**

```bash
uv run pytest tests/sources/test_types.py -v
```

Expected: `ModuleNotFoundError: No module named 'toolr.sources'`.

- [ ] **Step 3: Write minimal implementation**

`crates/toolr-py/python/toolr/sources/_types.py`:

```python
"""Schema types for externally-discovered toolr commands.

`ArgSchema` and `CommandSchema` are produced by the Rust argparse
scanner (or, in the future, by external source plugins) and shipped
through the manifest. They are also exposed on
`DispatchCommand.schema` so dispatcher bodies can reconstruct argv.
"""

from __future__ import annotations

from typing import Literal

from msgspec import Struct


class ArgSchema(Struct, frozen=True):
    """One argument on a discovered command.

    Mirrors the argparse `add_argument` fields the scanner can extract.
    Anything the scanner can't statically resolve is left at its default
    (`None`) and recorded as a warning at scan time.
    """

    name: str
    kind: Literal["positional", "optional", "flag", "repeated"]
    help: str = ""
    default: str | None = None
    choices: list[str] | None = None
    metavar: str | None = None
    type_annotation: str | None = None  # "str" / "int" / "float" / "bool"
    nargs: Literal["*", "+", "?"] | int | None = None
```

`crates/toolr-py/python/toolr/sources/__init__.py`:

```python
"""Public surface for externally-discovered toolr command schemas."""

from __future__ import annotations

from toolr.sources._types import ArgSchema

__all__ = ["ArgSchema"]
```

`tests/sources/__init__.py`: empty file.

- [ ] **Step 4: Run test to verify it passes**

```bash
uv run pytest tests/sources/test_types.py -v
```

Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-py/python/toolr/sources/ tests/sources/__init__.py tests/sources/test_types.py
git commit -m "sources: introduce ArgSchema for externally-discovered commands"
```

---

### Task 2: `CommandSchema`

**Files:**

- Modify: `crates/toolr-py/python/toolr/sources/_types.py`
- Modify: `crates/toolr-py/python/toolr/sources/__init__.py`
- Modify: `tests/sources/test_types.py`
- [ ] **Step 1: Add failing test**

Append to `tests/sources/test_types.py`:

```python
from toolr.sources import CommandSchema


def test_command_schema_holds_args_and_help():
    cmd = CommandSchema(
        name="migrate",
        summary="Updates database schema",
        description="Migrates the database.\nSupports rolling back.",
        arguments=[
            ArgSchema(name="app_label", kind="positional", help="App"),
            ArgSchema(name="check", kind="flag", help="Dry run"),
        ],
    )
    assert cmd.name == "migrate"
    assert len(cmd.arguments) == 2
    assert cmd.arguments[1].kind == "flag"


def test_command_schema_round_trips():
    cmd = CommandSchema(name="x", summary="", description="", arguments=[])
    decoded = msgspec.json.decode(msgspec.json.encode(cmd), type=CommandSchema)
    assert decoded == cmd
```

- [ ] **Step 2: Verify failure**

```bash
uv run pytest tests/sources/test_types.py -v
```

Expected: `ImportError: cannot import name 'CommandSchema'`.

- [ ] **Step 3: Implement**

Append to `crates/toolr-py/python/toolr/sources/_types.py`:

```python
class CommandSchema(Struct, frozen=True):
    """One command discovered by the argparse scanner.

    `arguments` carries only the command-specific args. Hoisted
    common_args (declared in `[tool.toolr.argparse.<name>]`) are
    applied at attach time and merged with `arguments`; consumers see
    a single combined list on the manifest side.
    """

    name: str
    summary: str
    description: str
    arguments: list[ArgSchema]
```

Update `__init__.py`:

```python
from toolr.sources._types import ArgSchema, CommandSchema

__all__ = ["ArgSchema", "CommandSchema"]
```

- [ ] **Step 4: Verify**

```bash
uv run pytest tests/sources/test_types.py -v
```

Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-py/python/toolr/sources/ tests/sources/test_types.py
git commit -m "sources: add CommandSchema"
```

---

### Task 3: `DispatchCommand` shape

**Files:**

- Create: `crates/toolr-py/python/toolr/sources/_dispatch.py`
- Create: `tests/sources/test_dispatch.py`
- Modify: `crates/toolr-py/python/toolr/sources/__init__.py`
- [ ] **Step 1: Write failing test**

`tests/sources/test_dispatch.py`:

```python
"""Tests for DispatchCommand basic shape (argv tested separately)."""

from __future__ import annotations

from toolr.sources import ArgSchema, CommandSchema, DispatchCommand


def _migrate_schema() -> CommandSchema:
    return CommandSchema(
        name="migrate",
        summary="",
        description="",
        arguments=[
            ArgSchema(name="check", kind="flag", help=""),
            ArgSchema(name="database", kind="optional", help="", default="default"),
        ],
    )


def test_dispatch_command_holds_match():
    dc = DispatchCommand(
        command="migrate",
        command_args={"check": True, "database": "primary"},
        schema=_migrate_schema(),
    )
    assert dc.command == "migrate"
    assert dc.command_args == {"check": True, "database": "primary"}
    assert dc.schema.name == "migrate"
```

- [ ] **Step 2: Verify failure**

```bash
uv run pytest tests/sources/test_dispatch.py -v
```

Expected: ImportError on `DispatchCommand`.

- [ ] **Step 3: Implement**

`crates/toolr-py/python/toolr/sources/_dispatch.py`:

```python
"""DispatchCommand — the runtime payload injected into dispatcher commands.

A dispatcher command is a user-written toolr command whose signature
declares exactly one keyword-only parameter annotated as
DispatchCommand. When the runtime matches one of the dispatcher's
attached children, it constructs this object and passes it in as the
value of that parameter.

`argv` reconstructs argparse-shaped argv — typically used by the
dispatcher body to forward to a subprocess (e.g.
`ctx.run('python', 'manage.py', *dispatched.argv)`).
"""

from __future__ import annotations

from typing import Any

from msgspec import Struct

from toolr.sources._types import CommandSchema


class DispatchCommand(Struct, frozen=True):
    command: str
    command_args: dict[str, Any]
    schema: CommandSchema
```

Update `__init__.py`:

```python
from toolr.sources._dispatch import DispatchCommand
from toolr.sources._types import ArgSchema, CommandSchema

__all__ = ["ArgSchema", "CommandSchema", "DispatchCommand"]
```

- [ ] **Step 4: Verify**

```bash
uv run pytest tests/sources/test_dispatch.py -v
```

Expected: 1 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-py/python/toolr/sources/ tests/sources/test_dispatch.py
git commit -m "sources: add DispatchCommand"
```

---

### Task 4: `DispatchCommand.argv` reconstruction

**Files:**

- Modify: `crates/toolr-py/python/toolr/sources/_dispatch.py`
- Modify: `tests/sources/test_dispatch.py`
- [ ] **Step 1: Add table-driven failing tests**

Append to `tests/sources/test_dispatch.py`:

```python
import pytest


@pytest.mark.parametrize(
    ("args_in", "schema_args", "expected"),
    [
        # Positional value.
        (
            {"app_label": "auth"},
            [ArgSchema(name="app_label", kind="positional", help="")],
            ["auth"],
        ),
        # Flag set True → emit, False → omit.
        (
            {"check": True, "verbose": False},
            [
                ArgSchema(name="check", kind="flag", help=""),
                ArgSchema(name="verbose", kind="flag", help=""),
            ],
            ["--check"],
        ),
        # Optional with default — omit when equal, emit otherwise.
        (
            {"database": "default"},
            [ArgSchema(name="database", kind="optional", help="", default="default")],
            [],
        ),
        (
            {"database": "primary"},
            [ArgSchema(name="database", kind="optional", help="", default="default")],
            ["--database", "primary"],
        ),
        # Repeated → one `--name value` per element.
        (
            {"exclude": ["a", "b"]},
            [ArgSchema(name="exclude", kind="repeated", help="")],
            ["--exclude", "a", "--exclude", "b"],
        ),
        # Underscores in the name become dashes on the wire.
        (
            {"dry_run": True},
            [ArgSchema(name="dry_run", kind="flag", help="")],
            ["--dry-run"],
        ),
    ],
)
def test_argv_reconstruction(args_in, schema_args, expected):
    schema = CommandSchema(name="x", summary="", description="", arguments=schema_args)
    dc = DispatchCommand(command="x", command_args=args_in, schema=schema)
    assert dc.argv == expected


def test_argv_unknown_arg_name_raises():
    schema = CommandSchema(name="x", summary="", description="", arguments=[])
    dc = DispatchCommand(command="x", command_args={"surprise": True}, schema=schema)
    with pytest.raises(ValueError, match="surprise"):
        _ = dc.argv
```

- [ ] **Step 2: Verify failure**

```bash
uv run pytest tests/sources/test_dispatch.py -v
```

Expected: failures on `AttributeError: ... has no attribute 'argv'`.

- [ ] **Step 3: Implement**

Replace the body of `crates/toolr-py/python/toolr/sources/_dispatch.py` with:

```python
"""DispatchCommand — the runtime payload injected into dispatcher commands."""

from __future__ import annotations

from typing import Any

from msgspec import Struct

from toolr.sources._types import CommandSchema


def _flag_for(name: str) -> str:
    """`dry_run` → `--dry-run`."""
    return "--" + name.replace("_", "-")


class DispatchCommand(Struct, frozen=True):
    command: str
    command_args: dict[str, Any]
    schema: CommandSchema

    @property
    def argv(self) -> list[str]:
        """Argparse-shaped argv reconstructed from `command_args` per `schema`.

        For each argument in `schema.arguments` that appears in
        `command_args`, emit the appropriate token(s):

        - `positional` → bare value
        - `flag` → `--name` when truthy, omitted when falsy
        - `optional` → `--name value`, omitted when value == default
        - `repeated` → `--name value` per element

        Keys in `command_args` not found in `schema.arguments` raise
        ValueError so typos surface loudly.
        """
        known = {a.name for a in self.schema.arguments}
        for key in self.command_args:
            if key not in known:
                msg = (
                    f"DispatchCommand.argv: unknown argument {key!r} "
                    "(not in schema)"
                )
                raise ValueError(msg)

        out: list[str] = []
        for arg in self.schema.arguments:
            if arg.name not in self.command_args:
                continue
            value = self.command_args[arg.name]
            if arg.kind == "positional":
                out.append(str(value))
            elif arg.kind == "flag":
                if value:
                    out.append(_flag_for(arg.name))
            elif arg.kind == "optional":
                if arg.default is not None and str(value) == arg.default:
                    continue
                out.extend([_flag_for(arg.name), str(value)])
            elif arg.kind == "repeated":
                for element in value:
                    out.extend([_flag_for(arg.name), str(element)])
        return out
```

- [ ] **Step 4: Verify**

```bash
uv run pytest tests/sources/test_dispatch.py -v
```

Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-py/python/toolr/sources/_dispatch.py tests/sources/test_dispatch.py
git commit -m "sources: implement DispatchCommand.argv reconstruction"
```

---

### Task 5: Confirm public surface

**Files:**

- Create: `tests/sources/test_public_surface.py`

- [ ] **Step 1: Write test**

`tests/sources/test_public_surface.py`:

```python
"""toolr.sources should re-export exactly the documented public surface."""

from __future__ import annotations

import toolr.sources


EXPECTED = {"ArgSchema", "CommandSchema", "DispatchCommand"}


def test_all_lists_public_surface():
    assert set(toolr.sources.__all__) == EXPECTED


def test_each_name_is_importable():
    for name in EXPECTED:
        assert hasattr(toolr.sources, name), f"missing: {name}"
```

- [ ] **Step 2: Run**

```bash
uv run pytest tests/sources/test_public_surface.py -v
```

Expected: 2 passed.

- [ ] **Step 3: Commit**

```bash
git add tests/sources/test_public_surface.py
git commit -m "sources: lock down toolr.sources public surface"
```

---

## Phase 2 — Detection rule

### Task 6: Dispatcher detection in signature parser

**Files:**

- Modify: `crates/toolr-py/python/toolr/utils/_signature.py`
- Create: `tests/sources/test_detection.py`
- [ ] **Step 1: Write failing test**

`tests/sources/test_detection.py`:

```python
"""Tests for the DispatchCommand-based dispatcher detection rule."""

from __future__ import annotations

import pytest

from toolr.sources import DispatchCommand
from toolr.utils._signature import (
    DispatcherDetectionError,
    detect_dispatch_parameter,
)


def test_normal_command_returns_none():
    def cmd(ctx, *, name: str = "x") -> None: ...

    assert detect_dispatch_parameter(cmd) is None


def test_dispatcher_returns_parameter_name():
    def cmd(ctx, *, cpu: str = "1", dispatched: DispatchCommand) -> None: ...

    assert detect_dispatch_parameter(cmd) == "dispatched"


def test_dispatcher_param_name_is_free():
    def cmd(ctx, *, target: DispatchCommand) -> None: ...

    assert detect_dispatch_parameter(cmd) == "target"


def test_multiple_dispatchcommand_params_raises():
    def cmd(ctx, *, a: DispatchCommand, b: DispatchCommand) -> None: ...

    with pytest.raises(DispatcherDetectionError, match="more than one"):
        detect_dispatch_parameter(cmd)


def test_dispatchcommand_in_positional_raises():
    def cmd(ctx, dispatched: DispatchCommand) -> None: ...

    with pytest.raises(DispatcherDetectionError, match="keyword-only"):
        detect_dispatch_parameter(cmd)
```

- [ ] **Step 2: Verify failure**

```bash
uv run pytest tests/sources/test_detection.py -v
```

Expected: ImportError on `detect_dispatch_parameter`.

- [ ] **Step 3: Implement**

Append to `crates/toolr-py/python/toolr/utils/_signature.py` (module-level, after existing class definitions):

```python
class DispatcherDetectionError(Exception):
    """Raised when a function's DispatchCommand usage is malformed."""


def detect_dispatch_parameter(func: Callable[..., Any]) -> str | None:
    """Return the name of the function's `DispatchCommand` parameter, or None.

    A command qualifies as a dispatcher iff exactly one keyword-only
    parameter is annotated with `toolr.sources.DispatchCommand`. The
    parameter name itself is free. Subclasses are not supported in v1.
    Returns `None` when the function isn't a dispatcher; raises
    `DispatcherDetectionError` on a malformed usage.
    """
    # Local import: keep toolr.sources out of the import-time graph of
    # toolr.utils._signature.
    from toolr.sources import DispatchCommand

    sig = inspect.signature(func)
    found_kw: list[str] = []
    for name, param in sig.parameters.items():
        annotation = param.annotation
        if annotation is inspect.Parameter.empty:
            continue
        if annotation is not DispatchCommand:
            continue
        if param.kind != inspect.Parameter.KEYWORD_ONLY:
            msg = (
                f"DispatchCommand parameter {name!r} on {func.__qualname__!r} "
                "must be keyword-only"
            )
            raise DispatcherDetectionError(msg)
        found_kw.append(name)

    if len(found_kw) > 1:
        msg = (
            f"{func.__qualname__!r} declares more than one DispatchCommand "
            f"parameter: {found_kw}"
        )
        raise DispatcherDetectionError(msg)
    return found_kw[0] if found_kw else None
```

`import inspect` is already at the top of `_signature.py`; if not, add it.

- [ ] **Step 4: Verify**

```bash
uv run pytest tests/sources/test_detection.py -v
```

Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-py/python/toolr/utils/_signature.py tests/sources/test_detection.py
git commit -m "sources: detect dispatcher commands via DispatchCommand annotation"
```

---

## Phase 3 — `dispatched_from` field

### Task 7: Add `dispatched_from` field to `Command`

**Files:**

- Modify: `crates/toolr-core/src/manifest/model.rs`

**Background:** Pre-1.0 toolr means no schema version bump is needed. `serde(default, skip_serializing_if = "Option::is_none")` makes the new field forward- and backward-compatible at the JSON layer.

- [ ] **Step 1: Write failing tests**

Add to the existing `#[cfg(test)] mod tests {…}` block in `crates/toolr-core/src/manifest/model.rs`, or create one at the bottom of the file:

```rust
#[cfg(test)]
mod dispatched_from_tests {
    use super::*;

    fn cmd_with(dispatched_from: Option<String>) -> Command {
        Command {
            name: "migrate".into(),
            group: "django".into(),
            module: "tools.django_dispatcher".into(),
            function: "django".into(),
            summary: String::new(),
            description: String::new(),
            arguments: vec![],
            imports: vec![],
            origin: Origin::default(),
            dispatched_from,
        }
    }

    #[test]
    fn command_serializes_dispatched_from_when_present() {
        let json = serde_json::to_string(&cmd_with(Some("argparse:django".into()))).unwrap();
        assert!(json.contains(r#""dispatched_from":"argparse:django""#));
    }

    #[test]
    fn command_omits_dispatched_from_when_none() {
        let json = serde_json::to_string(&cmd_with(None)).unwrap();
        assert!(!json.contains("dispatched_from"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p toolr-core --quiet dispatched_from
```

Expected: compile error (`Command` has no field `dispatched_from`).

- [ ] **Step 3: Implement**

Add to the `Command` struct definition (right before its closing brace):

```rust
    /// Source identifier (e.g. `"argparse:django"`) when this command
    /// was grafted from an external source. The runtime treats commands
    /// with this set as dispatched leaves: the parent's `target` is
    /// invoked with a constructed `DispatchCommand` payload, not as a
    /// regular command call. `None` for normal commands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatched_from: Option<String>,
```

Update every other place that constructs a `Command` literal:

```bash
rg -n 'Command \{' crates/
```

Each such site needs `dispatched_from: None,` added. Most are likely test fixtures.

- [ ] **Step 4: Verify**

```bash
cargo test -p toolr-core --quiet
cargo build --workspace --tests
```

Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/manifest/model.rs
git commit -m "manifest: add optional dispatched_from field on Command"
```

---

## Phase 4 — Argparse scanner (Rust)

### Task 8: Module scaffold + config parsing

**Files:**

- Create: `crates/toolr-core/src/argparse/mod.rs`
- Create: `crates/toolr-core/src/argparse/config.rs`
- Modify: `crates/toolr-core/src/lib.rs`
- [ ] **Step 1: Write failing test**

In `crates/toolr-core/src/argparse/config.rs` (at the bottom, with `#[cfg(test)]`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_one_block_two_attachments() {
        let toml_text = r#"
            [tool.toolr.argparse.django]
            scan_paths = ["apps/*/management/commands/*.py"]
            common_args = [
              { name = "verbosity", kind = "optional", default = "1" },
            ]

            [[tool.toolr.argparse.django.attach]]
            parent = "django"

            [[tool.toolr.argparse.django.attach]]
            parent = "jenkins.job"
        "#;
        let blocks = parse_blocks(toml_text).unwrap();
        assert_eq!(blocks.len(), 1);
        let block = &blocks[0];
        assert_eq!(block.name, "django");
        assert_eq!(block.scan_paths, vec!["apps/*/management/commands/*.py"]);
        assert_eq!(block.common_args.len(), 1);
        assert_eq!(
            block.attachments.iter().map(|a| a.parent.as_str()).collect::<Vec<_>>(),
            vec!["django", "jenkins.job"],
        );
    }

    #[test]
    fn empty_table_returns_empty() {
        assert!(parse_blocks("[project]\nname = 'x'\n").unwrap().is_empty());
    }
}
```

- [ ] **Step 2: Verify failure**

```bash
cargo test -p toolr-core --quiet argparse::config
```

Expected: unresolved module `argparse`.

- [ ] **Step 3: Implement**

`crates/toolr-core/src/argparse/mod.rs`:

```rust
//! Built-in argparse scanner: AST-walks Python files declared in
//! `[tool.toolr.argparse.*]` and grafts their `parser.add_argument`
//! calls as manifest children of user-declared dispatcher commands.

pub mod attach;
pub mod config;
pub mod scan;

pub use attach::graft_children;
pub use config::{ArgparseBlock, Attachment, parse_blocks, parse_blocks_from_pyproject};
pub use scan::scan_file;
```

`crates/toolr-core/src/argparse/config.rs`:

```rust
//! Parse `[tool.toolr.argparse.*]` blocks from `tools/pyproject.toml`.

use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

use crate::manifest::ArgumentKind;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ArgparseBlock {
    /// Block key under `[tool.toolr.argparse.<name>]`. Set by the parser
    /// from the table key, not from a field on the table.
    #[serde(skip_deserializing, default)]
    pub name: String,
    #[serde(default)]
    pub scan_paths: Vec<String>,
    #[serde(default)]
    pub common_args: Vec<CommonArg>,
    #[serde(default)]
    pub attach: Vec<Attachment>,
}

impl ArgparseBlock {
    pub fn attachments(&self) -> &[Attachment] {
        &self.attach
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CommonArg {
    pub name: String,
    pub kind: ArgumentKind,
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub choices: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Attachment {
    pub parent: String,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to parse tool.toolr.argparse: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("failed to read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Public: parse blocks from a raw TOML string. Convenient for tests.
pub fn parse_blocks(toml_text: &str) -> Result<Vec<ArgparseBlock>, ConfigError> {
    #[derive(Deserialize)]
    struct Root {
        #[serde(default)]
        tool: Tool,
    }
    #[derive(Default, Deserialize)]
    struct Tool {
        #[serde(default)]
        toolr: Toolr,
    }
    #[derive(Default, Deserialize)]
    struct Toolr {
        #[serde(default)]
        argparse: std::collections::BTreeMap<String, ArgparseBlock>,
    }
    let root: Root = toml::from_str(toml_text)?;
    Ok(root
        .tool
        .toolr
        .argparse
        .into_iter()
        .map(|(name, mut block)| {
            block.name = name;
            block
        })
        .collect())
}

/// Public: read `pyproject.toml` from disk and parse.
pub fn parse_blocks_from_pyproject(
    pyproject: &Path,
) -> Result<Vec<ArgparseBlock>, ConfigError> {
    let text = std::fs::read_to_string(pyproject).map_err(|source| ConfigError::Io {
        path: pyproject.display().to_string(),
        source,
    })?;
    parse_blocks(&text)
}
```

`crates/toolr-core/src/argparse/scan.rs` and `attach.rs`: create empty modules for now so `mod.rs` compiles:

```rust
//! Placeholder — implemented in later tasks.
pub fn scan_file() {} // remove in Task 9
```

```rust
//! Placeholder — implemented in later tasks.
pub fn graft_children() {} // remove in Task 13
```

Add to `crates/toolr-core/src/lib.rs`:

```rust
pub mod argparse;
```

Also ensure `toml` (with `derive` feature) is in `toolr-core`'s `Cargo.toml` dependencies (it likely is via the workspace).

- [ ] **Step 4: Verify**

```bash
cargo test -p toolr-core --quiet argparse::config
```

Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/argparse/ crates/toolr-core/src/lib.rs
git commit -m "argparse: scaffold module + parse [tool.toolr.argparse.*] blocks"
```

---

### Task 9: AST helper — extract `add_argument(...)` calls from one file

**Files:**

- Modify: `crates/toolr-core/src/argparse/scan.rs`

**Background:** Find every call expression whose function is `<anything>.add_argument` and extract structured info about each. We use the existing `ruff_python_parser` workspace dep.

- [ ] **Step 1: Write failing test**

Create `crates/toolr-core/src/argparse/scan.rs`:

```rust
//! Walk a Python file's AST and extract `<x>.add_argument(...)` calls.

use ruff_python_ast as ast;
use ruff_python_parser as parser;
use thiserror::Error;

use crate::argparse::config::CommonArg;
use crate::manifest::{Argument, ArgumentKind};

#[derive(Debug, Error)]
pub enum ScanError {
    #[error("failed to parse {path}: {message}")]
    Parse { path: String, message: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScannedCommand {
    pub name: String,        // filename stem
    pub summary: String,     // first paragraph of module docstring
    pub description: String, // rest of module docstring
    pub arguments: Vec<Argument>,
    pub warnings: Vec<String>,
}

/// Parse `source_text` (Python) and return a `ScannedCommand`.
/// `command_name` is what the caller wants to label this file's discovered
/// command as (typically the filename stem).
pub fn scan_source(command_name: &str, source_text: &str) -> Result<ScannedCommand, ScanError> {
    todo!("Task 9")
}
```

Then in the same file's `#[cfg(test)]`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_positional_optional_flag_and_repeated() {
        let source = r#"
"""Migrate the database.

Handles schema migrations and rolling back.
"""
def add_arguments(self, parser):
    parser.add_argument('app_label')
    parser.add_argument('--database', default='default', help='Target DB')
    parser.add_argument('--check', action='store_true', help='Dry run')
    parser.add_argument('--exclude', action='append')
"#;
        let scanned = scan_source("migrate", source).unwrap();
        assert_eq!(scanned.name, "migrate");
        assert_eq!(scanned.summary, "Migrate the database.");
        assert!(scanned.description.contains("Handles schema migrations"));

        let names: Vec<_> = scanned.arguments.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["app_label", "database", "check", "exclude"]);

        assert_eq!(scanned.arguments[0].kind, ArgumentKind::Positional);
        assert_eq!(scanned.arguments[1].kind, ArgumentKind::Optional);
        assert_eq!(scanned.arguments[1].default.as_deref(), Some("default"));
        assert_eq!(scanned.arguments[2].kind, ArgumentKind::Flag);
        assert_eq!(scanned.arguments[3].kind, ArgumentKind::Repeated);
    }

    #[test]
    fn empty_file_yields_command_with_no_args() {
        let scanned = scan_source("empty", "").unwrap();
        assert!(scanned.arguments.is_empty());
        assert_eq!(scanned.name, "empty");
    }

    #[test]
    fn unresolvable_type_emits_warning_and_no_type_annotation() {
        let source = r#"
def add_arguments(self, parser):
    parser.add_argument('--count', type=parse_count)
"#;
        let scanned = scan_source("x", source).unwrap();
        assert_eq!(scanned.arguments.len(), 1);
        assert_eq!(scanned.arguments[0].type_annotation, None);
        assert!(scanned.warnings.iter().any(|w| w.contains("type=")));
    }
}
```

- [ ] **Step 2: Verify failure**

```bash
cargo test -p toolr-core --quiet argparse::scan
```

Expected: panic on `todo!`.

- [ ] **Step 3: Implement**

Replace the body of `scan_source` with the real walker. Sketch (fill in the obvious bits):

```rust
pub fn scan_source(command_name: &str, source_text: &str) -> Result<ScannedCommand, ScanError> {
    let parsed = parser::parse_module(source_text)
        .map_err(|err| ScanError::Parse { path: command_name.into(), message: err.to_string() })?;
    let module = parsed.into_syntax();

    let mut out = ScannedCommand {
        name: command_name.to_string(),
        ..Default::default()
    };

    // Pull the module docstring (first expression statement that's a
    // string literal) into `summary` + `description`.
    if let Some(docstring) = module_docstring(&module) {
        let (head, rest) = split_first_paragraph(&docstring);
        out.summary = head;
        out.description = rest;
    }

    // Walk every Call expression looking for `<x>.add_argument(...)`.
    for call in find_add_argument_calls(&module) {
        match argument_from_call(call) {
            Ok((arg, warnings)) => {
                out.arguments.push(arg);
                out.warnings.extend(warnings);
            }
            Err(warning) => out.warnings.push(warning),
        }
    }

    Ok(out)
}
```

`module_docstring`, `split_first_paragraph`, `find_add_argument_calls`, and `argument_from_call` are private helpers in the same file. `argument_from_call` is the bulk of the work:

- Inspect the first positional argument string literal to determine `kind`:
    - Starts with `--` → `Optional` or `Flag` (decided by `action=`).
    - Starts with `-` followed by a single character → still `Optional`/`Flag` (short alias).
    - Otherwise → `Positional`.
- Look at `action=` (`store_true`/`store_false` → `Flag`; `append` → `Repeated`).
- Pull `default=`, `help=`, `choices=`, `type=`, `nargs=`, `metavar=` as best-effort string-encoded values. Anything not statically resolvable becomes a warning + `None`.

Use `ruff_python_ast` walkers / pattern matching against `Expr::Call` and the `Attribute` access on `.func`. See the existing usage in `crates/toolr-core/src/parser/` for the AST traversal idioms.

- [ ] **Step 4: Verify**

```bash
cargo test -p toolr-core --quiet argparse::scan
```

Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/argparse/scan.rs
git commit -m "argparse: AST-extract parser.add_argument calls from a Python source"
```

---

### Task 10: Per-file scan + glob expansion

**Files:**

- Modify: `crates/toolr-core/src/argparse/scan.rs`

- [ ] **Step 1: Add failing test**

Append to the `#[cfg(test)]` mod in `scan.rs`:

```rust
#[test]
fn scan_paths_expands_globs_and_skips_unparsable() {
    let project = tempfile::tempdir().unwrap();
    let cmds = project.path().join("apps/x/management/commands");
    std::fs::create_dir_all(&cmds).unwrap();
    std::fs::write(cmds.join("migrate.py"), "def add_arguments(self, parser):\n    parser.add_argument('app_label')\n").unwrap();
    std::fs::write(cmds.join("runserver.py"), "def add_arguments(self, parser):\n    parser.add_argument('--insecure', action='store_true')\n").unwrap();
    std::fs::write(cmds.join("broken.py"), "def add_arguments(self, parser:\n").unwrap(); // syntax error

    let scanned = scan_block_paths(
        project.path(),
        &["apps/*/management/commands/*.py".to_string()],
    ).unwrap();

    let mut names: Vec<_> = scanned.iter().map(|s| s.name.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["migrate", "runserver"]);
}
```

(If the `tempfile` dep isn't already used in `toolr-core` tests, add it to `[dev-dependencies]` in `crates/toolr-core/Cargo.toml`.)

- [ ] **Step 2: Verify failure**

```bash
cargo test -p toolr-core --quiet scan_paths_expands_globs
```

Expected: missing function `scan_block_paths`.

- [ ] **Step 3: Implement**

Add to `scan.rs`:

```rust
use std::path::Path;

/// Expand every glob in `scan_paths` against `root`, scan each match,
/// and return one `ScannedCommand` per file that parsed successfully.
/// Files that failed to parse become warnings on the returned list of
/// `ScannedCommand`s (each ScannedCommand carries its own
/// `warnings` field; parse errors that prevent any extraction are
/// emitted as a single-warning ScannedCommand with the failure path).
pub fn scan_block_paths(
    root: &Path,
    scan_paths: &[String],
) -> Result<Vec<ScannedCommand>, ScanError> {
    let mut all_paths: Vec<std::path::PathBuf> = Vec::new();
    for pattern in scan_paths {
        let abs = root.join(pattern);
        for entry in glob::glob(abs.to_str().unwrap_or_default())
            .map_err(|e| ScanError::Parse { path: pattern.clone(), message: e.to_string() })?
        {
            if let Ok(path) = entry {
                if path.is_file() {
                    all_paths.push(path);
                }
            }
        }
    }
    all_paths.sort();
    all_paths.dedup();

    let mut out = Vec::with_capacity(all_paths.len());
    for path in all_paths {
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(err) => {
                let mut placeholder = ScannedCommand::default();
                placeholder.name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?").into();
                placeholder.warnings.push(format!("failed to read {}: {}", path.display(), err));
                out.push(placeholder);
                continue;
            }
        };
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        match scan_source(stem, &text) {
            Ok(cmd) => out.push(cmd),
            Err(ScanError::Parse { message, .. }) => {
                let mut placeholder = ScannedCommand::default();
                placeholder.name = stem.into();
                placeholder.warnings.push(format!("failed to parse {}: {}", path.display(), message));
                out.push(placeholder);
            }
        }
    }
    Ok(out)
}
```

Add `glob = "0.3"` to `toolr-core`'s `Cargo.toml` if not already present (the workspace already pins it — reuse `workspace = true`).

- [ ] **Step 4: Verify**

```bash
cargo test -p toolr-core --quiet argparse::scan
```

Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/argparse/scan.rs crates/toolr-core/Cargo.toml
git commit -m "argparse: glob-expand scan_paths and scan each file"
```

---

### Task 11: Apply common_args to each scanned command

**Files:**

- Modify: `crates/toolr-core/src/argparse/scan.rs`

- [ ] **Step 1: Add failing test**

```rust
#[test]
fn common_args_are_appended_when_not_shadowed() {
    let scanned = ScannedCommand {
        name: "migrate".into(),
        summary: String::new(),
        description: String::new(),
        arguments: vec![Argument {
            name: "verbosity".into(),
            kind: ArgumentKind::Optional,
            help: "local".into(),
            default: Some("2".into()),
            type_annotation: None,
            resolved_type: None,
            allowed_values: vec![],
            path_constraints: None,
            metadata: Default::default(),
        }],
        warnings: vec![],
    };
    let common = vec![
        CommonArg { name: "verbosity".into(), kind: ArgumentKind::Optional, help: "common".into(), default: Some("0".into()), choices: None },
        CommonArg { name: "traceback".into(), kind: ArgumentKind::Flag, help: "tb".into(), default: None, choices: None },
    ];
    let merged = with_common_args(scanned, &common);
    let names: Vec<_> = merged.arguments.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, vec!["verbosity", "traceback"]);
    // The local "verbosity" wins.
    assert_eq!(merged.arguments[0].help, "local");
    assert_eq!(merged.arguments[0].default.as_deref(), Some("2"));
}
```

- [ ] **Step 2: Verify failure**

```bash
cargo test -p toolr-core --quiet common_args_are_appended
```

Expected: missing function `with_common_args`.

- [ ] **Step 3: Implement**

Append to `scan.rs`:

```rust
pub fn with_common_args(mut scanned: ScannedCommand, common: &[CommonArg]) -> ScannedCommand {
    let existing: std::collections::HashSet<&str> =
        scanned.arguments.iter().map(|a| a.name.as_str()).collect();
    let extras: Vec<Argument> = common
        .iter()
        .filter(|c| !existing.contains(c.name.as_str()))
        .map(|c| Argument {
            name: c.name.clone(),
            kind: c.kind,
            help: c.help.clone(),
            default: c.default.clone(),
            type_annotation: None,
            resolved_type: None,
            allowed_values: c.choices.clone().unwrap_or_default(),
            path_constraints: None,
            metadata: Default::default(),
        })
        .collect();
    scanned.arguments.extend(extras);
    scanned
}
```

- [ ] **Step 4: Verify**

```bash
cargo test -p toolr-core --quiet argparse
```

Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/argparse/scan.rs
git commit -m "argparse: apply common_args to each scanned command"
```

---

### Task 12: Validate attachments

**Files:**

- Create: `crates/toolr-core/src/argparse/attach.rs` (replace the placeholder)

- [ ] **Step 1: Write failing test**

`crates/toolr-core/src/argparse/attach.rs`:

```rust
//! Graft scanned commands under parent dispatchers, with validation.

use std::collections::HashMap;

use thiserror::Error;

use crate::argparse::config::ArgparseBlock;
use crate::argparse::scan::ScannedCommand;
use crate::manifest::Command;

#[derive(Debug, Error)]
pub enum AttachError {
    #[error("source {source!r} attaches to unknown parent {parent!r}{hint}")]
    UnknownParent { source: String, parent: String, hint: String },
    #[error("parent {parent!r} has no DispatchCommand-annotated keyword parameter")]
    NotADispatcher { parent: String },
    #[error("child name collision on parent {parent!r}: {name!r} is provided by both {a!r} and {b!r}")]
    Collision { parent: String, name: String, a: String, b: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parents() -> HashMap<String, (String, String)> {
        let mut m = HashMap::new();
        m.insert("django".into(), ("tools.dispatcher".into(), "django".into()));
        m
    }

    #[test]
    fn unknown_parent_with_hint() {
        let block = ArgparseBlock {
            name: "django".into(),
            scan_paths: vec![],
            common_args: vec![],
            attach: vec![crate::argparse::config::Attachment { parent: "djnago".into() }],
        };
        let err = validate_attachments(&[block], &parents()).unwrap_err();
        match err {
            AttachError::UnknownParent { hint, .. } => assert!(hint.contains("django")),
            e => panic!("unexpected: {e:?}"),
        }
    }

    #[test]
    fn collision_is_detected() {
        let children: HashMap<String, Vec<Command>> = HashMap::from([(
            "django".into(),
            vec![
                Command {
                    name: "migrate".into(), group: "django".into(),
                    module: "tools.dispatcher".into(), function: "django".into(),
                    summary: String::new(), description: String::new(),
                    arguments: vec![], imports: vec![], origin: Default::default(),
                    dispatched_from: Some("a".into()),
                },
                Command {
                    name: "migrate".into(), group: "django".into(),
                    module: "tools.dispatcher".into(), function: "django".into(),
                    summary: String::new(), description: String::new(),
                    arguments: vec![], imports: vec![], origin: Default::default(),
                    dispatched_from: Some("b".into()),
                },
            ],
        )]);
        let err = validate_no_collisions(&children).unwrap_err();
        assert!(matches!(err, AttachError::Collision { ref name, .. } if name == "migrate"));
    }
}
```

- [ ] **Step 2: Verify failure**

```bash
cargo test -p toolr-core --quiet argparse::attach
```

Expected: missing functions.

- [ ] **Step 3: Implement**

Append to `attach.rs`:

```rust
pub fn validate_attachments(
    blocks: &[ArgparseBlock],
    parents: &HashMap<String, (String, String)>,
) -> Result<(), AttachError> {
    for block in blocks {
        for attachment in &block.attach {
            if !parents.contains_key(&attachment.parent) {
                let hint = closest_parent_hint(&attachment.parent, parents.keys());
                return Err(AttachError::UnknownParent {
                    source: block.name.clone(),
                    parent: attachment.parent.clone(),
                    hint,
                });
            }
        }
    }
    Ok(())
}

pub fn validate_no_collisions(
    children_by_parent: &HashMap<String, Vec<Command>>,
) -> Result<(), AttachError> {
    for (parent, children) in children_by_parent {
        let mut seen: HashMap<&str, &str> = HashMap::new();
        for child in children {
            let source = child.dispatched_from.as_deref().unwrap_or("?");
            if let Some(prev_source) = seen.get(child.name.as_str()) {
                if *prev_source != source {
                    return Err(AttachError::Collision {
                        parent: parent.clone(),
                        name: child.name.clone(),
                        a: (*prev_source).into(),
                        b: source.into(),
                    });
                }
            }
            seen.insert(&child.name, source);
        }
    }
    Ok(())
}

fn closest_parent_hint<'a>(
    target: &str,
    candidates: impl Iterator<Item = &'a String>,
) -> String {
    use std::cmp::Ordering;

    let mut best: Option<(usize, &str)> = None;
    for candidate in candidates {
        let dist = edit_distance(target, candidate);
        if best.map_or(true, |(d, _)| dist < d) {
            best = Some((dist, candidate));
        }
    }
    match best {
        Some((d, name)) if d <= 3 => format!(" (did you mean {name:?}?)"),
        _ => String::new(),
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut prev = (0..=b.len()).collect::<Vec<_>>();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}
```

(Don't worry about the `Ordering` import if rustc says unused; remove it. The point is a tiny Levenshtein.)

- [ ] **Step 4: Verify**

```bash
cargo test -p toolr-core --quiet argparse::attach
```

Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/argparse/attach.rs
git commit -m "argparse: validate attachments and detect collisions"
```

---

### Task 13: Graft children under parents

**Files:**

- Modify: `crates/toolr-core/src/argparse/attach.rs`

- [ ] **Step 1: Add failing test**

Append to the `#[cfg(test)]` mod in `attach.rs`:

```rust
#[test]
fn graft_emits_one_child_per_scanned_with_dispatched_from() {
    let block = ArgparseBlock {
        name: "django".into(),
        scan_paths: vec![],
        common_args: vec![],
        attach: vec![crate::argparse::config::Attachment { parent: "django".into() }],
    };
    let scanned = vec![ScannedCommand {
        name: "migrate".into(),
        summary: "Migrate".into(),
        description: "".into(),
        arguments: vec![],
        warnings: vec![],
    }];
    let children = graft_children(
        &block,
        &scanned,
        &parents(),
    ).unwrap();
    assert_eq!(children.len(), 1);
    let django_children = children.get("django").unwrap();
    assert_eq!(django_children[0].name, "migrate");
    assert_eq!(django_children[0].dispatched_from.as_deref(), Some("argparse:django"));
    assert_eq!(django_children[0].module, "tools.dispatcher");
    assert_eq!(django_children[0].function, "django");
}
```

- [ ] **Step 2: Verify failure**

```bash
cargo test -p toolr-core --quiet argparse::attach
```

Expected: missing function `graft_children`.

- [ ] **Step 3: Implement**

```rust
use crate::manifest::Origin;

pub fn graft_children(
    block: &ArgparseBlock,
    scanned: &[ScannedCommand],
    parents: &HashMap<String, (String, String)>,
) -> Result<HashMap<String, Vec<Command>>, AttachError> {
    let mut out: HashMap<String, Vec<Command>> = HashMap::new();
    for attachment in &block.attach {
        let (module, function) = parents
            .get(&attachment.parent)
            .ok_or_else(|| AttachError::UnknownParent {
                source: block.name.clone(),
                parent: attachment.parent.clone(),
                hint: String::new(),
            })?;
        let entries = out.entry(attachment.parent.clone()).or_default();
        for sc in scanned {
            entries.push(Command {
                name: sc.name.clone(),
                group: attachment.parent.clone(),
                module: module.clone(),
                function: function.clone(),
                summary: sc.summary.clone(),
                description: sc.description.clone(),
                arguments: sc.arguments.clone(),
                imports: vec![],
                origin: Origin::default(),
                dispatched_from: Some(format!("argparse:{}", block.name)),
            });
        }
    }
    Ok(out)
}
```

- [ ] **Step 4: Verify**

```bash
cargo test -p toolr-core --quiet argparse::attach
```

Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/argparse/attach.rs
git commit -m "argparse: graft scanned commands under attached parents"
```

---

### Task 14: End-to-end scanner orchestration

**Files:**

- Modify: `crates/toolr-core/src/argparse/mod.rs`

- [ ] **Step 1: Add failing test**

In `mod.rs` `#[cfg(test)]`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn run_for_project_returns_grafted_children() {
        let project = tempfile::tempdir().unwrap();
        let tools = project.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        let cmds = project.path().join("apps/billing/management/commands");
        std::fs::create_dir_all(&cmds).unwrap();
        std::fs::write(cmds.join("sync.py"),
            "def add_arguments(self, parser):\n    parser.add_argument('--force', action='store_true')\n").unwrap();
        std::fs::write(tools.join("pyproject.toml"), r#"
            [tool.toolr.argparse.django]
            scan_paths = ["apps/*/management/commands/*.py"]

            [[tool.toolr.argparse.django.attach]]
            parent = "django"
        "#).unwrap();

        let mut parents = HashMap::new();
        parents.insert("django".to_string(), ("tools.dispatcher".to_string(), "django".to_string()));

        let result = run_for_project(project.path(), &parents).unwrap();
        let django_children = result.get("django").unwrap();
        assert_eq!(django_children.len(), 1);
        assert_eq!(django_children[0].name, "sync");
        assert_eq!(django_children[0].dispatched_from.as_deref(), Some("argparse:django"));
    }
}
```

- [ ] **Step 2: Verify failure**

```bash
cargo test -p toolr-core --quiet argparse::tests::run_for_project
```

Expected: missing function.

- [ ] **Step 3: Implement**

In `mod.rs`:

```rust
use std::collections::HashMap;
use std::path::Path;

use crate::manifest::Command;

#[derive(Debug, thiserror::Error)]
pub enum ArgparseError {
    #[error(transparent)]
    Config(#[from] config::ConfigError),
    #[error(transparent)]
    Scan(#[from] scan::ScanError),
    #[error(transparent)]
    Attach(#[from] attach::AttachError),
}

/// Run the full argparse pipeline for a project: read pyproject,
/// scan files, validate attachments, graft children, detect
/// collisions. Returns `{parent_dotted_name: [child Commands]}`.
///
/// `parents` is `{dotted_name: (module, function)}` for every user-
/// declared command that could be a dispatcher. The caller populates
/// this from the static + dynamic registry walks.
pub fn run_for_project(
    project_root: &Path,
    parents: &HashMap<String, (String, String)>,
) -> Result<HashMap<String, Vec<Command>>, ArgparseError> {
    let pyproject = project_root.join("tools").join("pyproject.toml");
    if !pyproject.exists() {
        return Ok(HashMap::new());
    }
    let blocks = config::parse_blocks_from_pyproject(&pyproject)?;
    attach::validate_attachments(&blocks, parents)?;

    let mut out: HashMap<String, Vec<Command>> = HashMap::new();
    for block in &blocks {
        let scanned: Vec<scan::ScannedCommand> = scan::scan_block_paths(project_root, &block.scan_paths)?
            .into_iter()
            .map(|s| scan::with_common_args(s, &block.common_args))
            .collect();
        for (parent, children) in attach::graft_children(block, &scanned, parents)? {
            out.entry(parent).or_default().extend(children);
        }
    }
    attach::validate_no_collisions(&out)?;
    Ok(out)
}
```

- [ ] **Step 4: Verify**

```bash
cargo test -p toolr-core --quiet
```

Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add crates/toolr-core/src/argparse/mod.rs
git commit -m "argparse: end-to-end run_for_project orchestrator"
```

---

## Phase 5 — Static-manifest integration

### Task 15: Hook `argparse::run_for_project` into `build_static_manifest`

**Files:**

- Modify: `crates/toolr-core/src/parser/build.rs` (or wherever the in-tree `build_static_manifest` lives — locate first).

**Background:** The static layer today walks `tools/*.py` for `@command` decorators. We add an argparse pass *after* the decorator walk, so parent dispatcher commands are already known when validation runs.

- [ ] **Step 1: Locate the seam**

```bash
rg -n 'fn build_static_manifest' crates/toolr-core/src/parser/
```

Find the end of the existing function — the point where it returns its assembled `Manifest`. The new code runs just before the return, mutating `manifest.commands` to extend with grafted children.

- [ ] **Step 2: Write a failing test**

In the same file as `build_static_manifest`, in `#[cfg(test)]`:

```rust
#[test]
fn build_static_manifest_grafts_argparse_children() {
    use tempfile::tempdir;
    let project = tempdir().unwrap();
    let tools = project.path().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::write(tools.join("__init__.py"), "").unwrap();
    std::fs::write(tools.join("dispatcher.py"), r#"
from toolr import command_group, Context
from toolr.sources import DispatchCommand

group = command_group("django", "Django")

@group.command
def django(ctx: Context, *, dispatched: DispatchCommand) -> int:
    return 0
"#).unwrap();
    let cmds = project.path().join("apps/x/management/commands");
    std::fs::create_dir_all(&cmds).unwrap();
    std::fs::write(cmds.join("migrate.py"),
        "def add_arguments(self, parser):\n    parser.add_argument('--check', action='store_true')\n").unwrap();
    std::fs::write(tools.join("pyproject.toml"), r#"
[tool.toolr.argparse.django]
scan_paths = ["apps/*/management/commands/*.py"]

[[tool.toolr.argparse.django.attach]]
parent = "django"
"#).unwrap();

    let manifest = build_static_manifest(&tools).unwrap();
    let names: std::collections::BTreeSet<_> = manifest.commands.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains("django"));
    assert!(names.contains("migrate"));
    let migrate = manifest.commands.iter().find(|c| c.name == "migrate").unwrap();
    assert_eq!(migrate.group, "django");
    assert_eq!(migrate.dispatched_from.as_deref(), Some("argparse:django"));
    let django = manifest.commands.iter().find(|c| c.name == "django").unwrap();
    assert_eq!(migrate.module, django.module);
    assert_eq!(migrate.function, django.function);
}
```

- [ ] **Step 3: Verify failure**

```bash
cargo test -p toolr-core --quiet build_static_manifest_grafts
```

Expected: `migrate` is not in `manifest.commands`.

- [ ] **Step 4: Implement**

In `build_static_manifest`, build a `parents` map from the freshly-walked manifest and call `argparse::run_for_project`:

```rust
// After the existing static walk produces `manifest`:

// Build the {dotted_parent -> (module, function)} map only from
// commands that could be dispatchers (have a function annotated
// with DispatchCommand). Since DispatchCommand annotation can't be
// detected from the Rust AST alone, we treat ALL commands as
// potential parents here and let later validation (Task 14's
// validate_attachments combined with dynamic-layer annotation
// check) handle the rest. The annotation check happens in
// `_introspect.py` for any [[attach]] entry.
let parents: std::collections::HashMap<String, (String, String)> = manifest
    .commands
    .iter()
    .map(|c| {
        let dotted = if c.group == "tools" {
            c.name.clone()
        } else {
            format!("{}.{}", c.group, c.name)
        };
        (dotted, (c.module.clone(), c.function.clone()))
    })
    .collect();

let project_root = tools_dir
    .parent()
    .ok_or_else(|| /* an existing toolr-core error variant */)?;

let grafted = crate::argparse::run_for_project(project_root, &parents)
    .map_err(/* map into the existing build error */)?;

for (_parent, mut children) in grafted {
    manifest.commands.append(&mut children);
}
```

The exact error-mapping and the dotted-name derivation depend on existing conventions — peek at the surrounding code for the right `BuildError` variants. If a fresh variant `Argparse(ArgparseError)` is cleanest, add it to the local error enum.

- [ ] **Step 5: Verify**

```bash
cargo test -p toolr-core --quiet
cargo build --workspace --tests
```

Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add crates/toolr-core/src/parser/build.rs
git commit -m "parser: run the argparse scanner inside build_static_manifest"
```

---

## Phase 6 — Runtime injection

### Task 16: Read `dispatched_from` and pack child kwargs (Rust)

**Files:**

- Modify: `crates/toolr/src/cli.rs`

**Background:** When the runtime invokes a matched leaf, it normally builds a kwargs dict from the leaf's clap matches and calls the user function. For dispatched leaves, the kwargs are split into "parent kwargs" (for the dispatcher's own flags) and "packed child" (for the matched child's args). The packed payload is later crossed into Python as a `DispatchCommand`.

- [ ] **Step 1: Find the invocation seam**

```bash
rg -n 'invoke|dispatch|matched_cmd|extract_arg' crates/toolr/src/cli.rs | head -20
```

Locate the existing code that runs after clap matching and before the Python call. Get familiar with the existing arg-extraction helper.

- [ ] **Step 2: Add a Rust test**

In `crates/toolr/src/cli.rs` `#[cfg(test)]` (extend existing or add):

```rust
#[cfg(test)]
mod dispatched_pack_tests {
    use super::*;
    use toolr_core::manifest::{Argument, ArgumentKind, Command, Origin};

    fn migrate_cmd() -> Command {
        Command {
            name: "migrate".into(),
            group: "django".into(),
            module: "tools.dispatcher".into(),
            function: "django".into(),
            summary: String::new(),
            description: String::new(),
            arguments: vec![Argument {
                name: "check".into(),
                kind: ArgumentKind::Flag,
                help: String::new(),
                default: None,
                type_annotation: None,
                resolved_type: None,
                allowed_values: vec![],
                path_constraints: None,
                metadata: Default::default(),
            }],
            imports: vec![],
            origin: Origin::default(),
            dispatched_from: Some("argparse:django".into()),
        }
    }

    #[test]
    fn pack_child_args_extracts_flag() {
        let cmd = clap::Command::new("migrate")
            .arg(clap::Arg::new("check").long("check").action(clap::ArgAction::SetTrue));
        let matches = cmd.try_get_matches_from(vec!["migrate", "--check"]).unwrap();
        let packed = pack_child_args(&migrate_cmd(), &matches);
        assert_eq!(packed.name, "migrate");
        assert_eq!(packed.args.get("check").map(|v| v.as_str()), Some("true"));
    }
}
```

- [ ] **Step 3: Verify failure**

```bash
cargo test -p toolr --quiet dispatched_pack
```

Expected: missing `pack_child_args` / `PackedChild`.

- [ ] **Step 4: Implement**

Add to `cli.rs`:

```rust
pub(crate) struct PackedChild {
    pub name: String,
    pub args: std::collections::BTreeMap<String, String>,
    pub schema: toolr_core::manifest::Command,
}

pub(crate) fn pack_child_args(
    cmd: &toolr_core::manifest::Command,
    matches: &clap::ArgMatches,
) -> PackedChild {
    let mut args = std::collections::BTreeMap::new();
    for arg in &cmd.arguments {
        if let Some(value) = extract_arg_string(matches, arg) {
            args.insert(arg.name.clone(), value);
        }
    }
    PackedChild {
        name: cmd.name.clone(),
        args,
        schema: cmd.clone(),
    }
}
```

`extract_arg_string` is the existing helper that already converts a single clap-matched arg into a String. Find it via grep and reuse. If it returns a richer type (e.g. `serde_json::Value`), keep that richer type instead of `String` and update `PackedChild.args` accordingly.

In the existing invocation seam, branch on `dispatched_from`:

```rust
if matched_cmd.dispatched_from.is_some() {
    let packed = pack_child_args(&matched_cmd, &leaf_matches);
    invoke_dispatcher_with_packed(&parent_cmd, &parent_matches, packed)?;
} else {
    // existing path unchanged
}
```

`invoke_dispatcher_with_packed` is added in Task 17.

- [ ] **Step 5: Verify**

```bash
cargo test -p toolr --quiet dispatched_pack
```

Expected: 1 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/toolr/src/cli.rs
git commit -m "cli: pack dispatched-child args for runtime injection"
```

---

### Task 17: Construct `DispatchCommand` and invoke the dispatcher

**Files:**

- Modify: `crates/toolr-py/python/toolr/_runner.py`
- Modify: `crates/toolr/src/cli.rs` (or sibling dispatch module)
- [ ] **Step 1: Find the Python invoker**

```bash
rg -n 'def run\|def invoke\|def dispatch' crates/toolr-py/python/toolr/_runner.py
```

Locate the existing function the pyo3 layer calls to invoke user commands.

- [ ] **Step 2: Add a failing test**

`tests/sources/test_runner_dispatch.py`:

```python
"""Runner-side: construct DispatchCommand and call the dispatcher function."""

from __future__ import annotations

from typing import Any

import pytest

from toolr._runner import invoke_dispatcher
from toolr.sources import ArgSchema, CommandSchema, DispatchCommand


def test_invoke_dispatcher_passes_dispatch_command():
    captured: dict[str, Any] = {}

    def parent(ctx, *, cpu: str = "1", dispatched: DispatchCommand) -> int:
        captured["cpu"] = cpu
        captured["dispatched"] = dispatched
        return 0

    schema = CommandSchema(
        name="migrate",
        summary="",
        description="",
        arguments=[ArgSchema(name="check", kind="flag", help="")],
    )
    rc = invoke_dispatcher(
        ctx=None,
        func=parent,
        parent_kwargs={"cpu": "5000m"},
        child_name="migrate",
        child_args={"check": True},
        child_schema=schema,
    )

    assert rc == 0
    assert captured["cpu"] == "5000m"
    assert isinstance(captured["dispatched"], DispatchCommand)
    assert captured["dispatched"].command == "migrate"
    assert captured["dispatched"].command_args == {"check": True}
    assert captured["dispatched"].schema == schema


def test_invoke_dispatcher_with_non_dispatcher_raises():
    def parent(ctx, *, cpu: str = "1") -> int: ...

    schema = CommandSchema(name="x", summary="", description="", arguments=[])
    with pytest.raises(RuntimeError, match="DispatchCommand"):
        invoke_dispatcher(
            ctx=None, func=parent, parent_kwargs={},
            child_name="x", child_args={}, child_schema=schema,
        )
```

- [ ] **Step 3: Verify failure**

```bash
uv run pytest tests/sources/test_runner_dispatch.py -v
```

Expected: ImportError on `invoke_dispatcher`.

- [ ] **Step 4: Implement (Python)**

Add to `crates/toolr-py/python/toolr/_runner.py`:

```python
from toolr.sources import CommandSchema, DispatchCommand
from toolr.utils._signature import detect_dispatch_parameter


def invoke_dispatcher(
    *,
    ctx: Any,
    func: Callable[..., Any],
    parent_kwargs: dict[str, Any],
    child_name: str,
    child_args: dict[str, Any],
    child_schema: CommandSchema,
) -> Any:
    """Call `func(ctx, **parent_kwargs, <dispatch_param>=DispatchCommand(...))`.

    Raises RuntimeError if `func` doesn't have a DispatchCommand-
    annotated parameter (manifest builder should have caught this at
    build time; this is a defensive guard against a stale manifest).
    """
    param = detect_dispatch_parameter(func)
    if param is None:
        raise RuntimeError(
            f"invoke_dispatcher: {func.__qualname__!r} has no "
            "DispatchCommand parameter (manifest out of sync?)"
        )
    dispatched = DispatchCommand(
        command=child_name,
        command_args=child_args,
        schema=child_schema,
    )
    return func(ctx, **parent_kwargs, **{param: dispatched})
```

- [ ] **Step 5: Wire the Rust side**

In `crates/toolr/src/cli.rs`, add `invoke_dispatcher_with_packed`:

```rust
fn invoke_dispatcher_with_packed(
    parent: &toolr_core::manifest::Command,
    parent_matches: &clap::ArgMatches,
    packed: PackedChild,
) -> anyhow::Result<()> {
    let parent_kwargs = build_parent_kwargs(parent, parent_matches);

    pyo3::Python::with_gil(|py| -> pyo3::PyResult<()> {
        let module = py.import("toolr._runner")?;
        let func = module.getattr("invoke_dispatcher")?;
        let kwargs = pyo3::types::PyDict::new(py);
        kwargs.set_item("ctx", build_context(py)?)?;
        kwargs.set_item("func", import_user_function(py, &parent.module, &parent.function)?)?;
        kwargs.set_item("parent_kwargs", parent_kwargs.into_py(py))?;
        kwargs.set_item("child_name", packed.name)?;
        kwargs.set_item("child_args", packed.args.into_py(py))?;
        kwargs.set_item(
            "child_schema",
            command_schema_to_pyobject(py, &packed.schema)?,
        )?;
        func.call((), Some(&kwargs))?;
        Ok(())
    })?;
    Ok(())
}
```

`build_context`, `import_user_function`, `build_parent_kwargs`, and `command_schema_to_pyobject` either exist as helpers or need a thin extraction. `command_schema_to_pyobject` constructs a `toolr.sources.CommandSchema` via pyo3 from the Rust-side `Command`. The cleanest way is via `py.import("toolr.sources")?.call_method1("CommandSchema", (name, summary, description, arguments_list))?`.

- [ ] **Step 6: Verify**

```bash
uv run pytest tests/sources/test_runner_dispatch.py -v
cargo test -p toolr --quiet
cargo build --workspace
```

Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add crates/toolr-py/python/toolr/_runner.py crates/toolr/src/cli.rs tests/sources/test_runner_dispatch.py
git commit -m "runtime: construct DispatchCommand and invoke parent on dispatched leaves"
```

---

## Phase 7 — E2E

### Task 18: End-to-end happy path (explicit rebuild)

**Files:**

- Create: `tests/sources/test_e2e.py`

- [ ] **Step 1: Write the test**

`tests/sources/test_e2e.py`:

```python
"""End-to-end: argparse scanner → rebuild → dispatch → assert payload."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import textwrap
from pathlib import Path

import pytest


@pytest.fixture
def project_with_dispatcher_and_command(tmp_path: Path) -> Path:
    """Tiny tools project plus an argparse-scanable management command."""
    project = tmp_path / "demo"
    project.mkdir()
    tools = project / "tools"
    tools.mkdir()
    (tools / "__init__.py").write_text("")
    (tools / "dispatcher.py").write_text(textwrap.dedent("""
        import json
        import os
        from toolr import command_group, Context
        from toolr.sources import DispatchCommand

        group = command_group("django", "Django")

        @group.command
        def django(ctx: Context, *, dispatched: DispatchCommand) -> int:
            payload = {
                "command": dispatched.command,
                "command_args": dispatched.command_args,
                "argv": dispatched.argv,
            }
            with open(os.environ["E2E_SIDECAR"], "w") as fh:
                json.dump(payload, fh)
            return 0
    """))

    cmds = project / "apps" / "billing" / "management" / "commands"
    cmds.mkdir(parents=True)
    (cmds / "migrate.py").write_text(textwrap.dedent("""
        \"\"\"Migrate the database.\"\"\"
        def add_arguments(self, parser):
            parser.add_argument('--check', action='store_true', help='Dry run')
            parser.add_argument('--database', default='default', help='Target DB')
    """))

    (tools / "pyproject.toml").write_text(textwrap.dedent("""
        [tool.toolr.argparse.django]
        scan_paths = ["apps/*/management/commands/*.py"]

        [[tool.toolr.argparse.django.attach]]
        parent = "django"
    """))
    return project


def test_e2e_dispatch_through_argparse_scanner(project_with_dispatcher_and_command, tmp_path: Path):
    project = project_with_dispatcher_and_command
    sidecar = tmp_path / "captured.json"

    # 1. Explicit rebuild.
    subprocess.run(
        ["toolr", "project", "manifest", "rebuild"],
        check=True, cwd=project,
    )

    # 2. Invoke through the dispatcher.
    env = {**os.environ, "E2E_SIDECAR": str(sidecar)}
    subprocess.run(
        ["toolr", "django", "migrate", "--check", "--database", "primary"],
        check=True, cwd=project, env=env,
    )

    # 3. Assert payload.
    captured = json.loads(sidecar.read_text())
    assert captured["command"] == "migrate"
    assert captured["command_args"]["check"] is True
    assert captured["command_args"]["database"] == "primary"
    assert "--check" in captured["argv"]
    assert "--database" in captured["argv"]
    assert "primary" in captured["argv"]
```

- [ ] **Step 2: Run**

```bash
uv run pytest tests/sources/test_e2e.py -v
```

Expected: pass.

- [ ] **Step 3: Commit**

```bash
git add tests/sources/test_e2e.py
git commit -m "tests: e2e — explicit rebuild then dispatch via argparse-scanned child"
```

---

### Task 19: E2E — multi-attachment + collision detection

**Files:**

- Modify: `tests/sources/test_e2e.py`

- [ ] **Step 1: Add tests**

Append:

```python
def test_e2e_same_source_attached_to_two_parents(tmp_path: Path):
    project = tmp_path / "demo2"
    project.mkdir()
    tools = project / "tools"
    tools.mkdir()
    (tools / "__init__.py").write_text("")
    (tools / "dispatcher.py").write_text(textwrap.dedent("""
        from toolr import command_group, Context
        from toolr.sources import DispatchCommand

        django_grp  = command_group("django", "Django")
        jenkins_grp = command_group("jenkins", "Jenkins")

        @django_grp.command
        def django(ctx: Context, *, dispatched: DispatchCommand) -> int:
            print(f"local:{dispatched.command}")
            return 0

        @jenkins_grp.command
        def job(ctx: Context, *, cpu: str = "1000m", dispatched: DispatchCommand) -> int:
            print(f"jenkins({cpu}):{dispatched.command}")
            return 0
    """))
    cmds = project / "apps" / "billing" / "management" / "commands"
    cmds.mkdir(parents=True)
    (cmds / "migrate.py").write_text(
        "def add_arguments(self, parser):\n    parser.add_argument('--check', action='store_true')\n"
    )
    (tools / "pyproject.toml").write_text(textwrap.dedent("""
        [tool.toolr.argparse.django]
        scan_paths = ["apps/*/management/commands/*.py"]

        [[tool.toolr.argparse.django.attach]]
        parent = "django"

        [[tool.toolr.argparse.django.attach]]
        parent = "jenkins.job"
    """))

    subprocess.run(["toolr", "project", "manifest", "rebuild"], check=True, cwd=project)

    out1 = subprocess.run(
        ["toolr", "django", "migrate"],
        check=True, capture_output=True, text=True, cwd=project,
    ).stdout.strip()
    out2 = subprocess.run(
        ["toolr", "jenkins", "job", "--cpu", "5000m", "migrate"],
        check=True, capture_output=True, text=True, cwd=project,
    ).stdout.strip()

    assert out1 == "local:migrate"
    assert out2 == "jenkins(5000m):migrate"


def test_e2e_collision_across_sources_fails_build(tmp_path: Path):
    project = tmp_path / "demo3"
    project.mkdir()
    tools = project / "tools"
    tools.mkdir()
    (tools / "__init__.py").write_text("")
    (tools / "dispatcher.py").write_text(textwrap.dedent("""
        from toolr import command_group, Context
        from toolr.sources import DispatchCommand

        group = command_group("django", "Django")

        @group.command
        def django(ctx: Context, *, dispatched: DispatchCommand) -> int:
            return 0
    """))
    cmds_a = project / "apps" / "a" / "management" / "commands"
    cmds_a.mkdir(parents=True)
    (cmds_a / "migrate.py").write_text(
        "def add_arguments(self, parser):\n    parser.add_argument('--a', action='store_true')\n"
    )
    cmds_b = project / "apps" / "b" / "management" / "commands"
    cmds_b.mkdir(parents=True)
    (cmds_b / "migrate.py").write_text(
        "def add_arguments(self, parser):\n    parser.add_argument('--b', action='store_true')\n"
    )
    (tools / "pyproject.toml").write_text(textwrap.dedent("""
        [tool.toolr.argparse.first]
        scan_paths = ["apps/a/management/commands/*.py"]
        [[tool.toolr.argparse.first.attach]]
        parent = "django"

        [tool.toolr.argparse.second]
        scan_paths = ["apps/b/management/commands/*.py"]
        [[tool.toolr.argparse.second.attach]]
        parent = "django"
    """))

    result = subprocess.run(
        ["toolr", "project", "manifest", "rebuild"],
        cwd=project, capture_output=True, text=True,
    )
    assert result.returncode != 0
    assert "migrate" in (result.stderr + result.stdout)
```

- [ ] **Step 2: Run**

```bash
uv run pytest tests/sources/test_e2e.py -v
```

Expected: 3 passed (one from Task 18 plus two new).

- [ ] **Step 3: Commit**

```bash
git add tests/sources/test_e2e.py
git commit -m "tests: e2e multi-attach + collision detection"
```

---

### Task 20: E2E — auto-rebuild path picks up sources too

**Files:**

- Modify: `tests/sources/test_e2e.py`

**Background:** With `.toolr-manifest.json` deleted (or never created), the next `toolr <command>` invocation auto-rebuilds via the static layer — which now includes the argparse scanner. This test asserts that.

- [ ] **Step 1: Add a test**

```python
def test_e2e_auto_rebuild_runs_argparse(tmp_path: Path):
    project = tmp_path / "demo_auto"
    project.mkdir()
    tools = project / "tools"
    tools.mkdir()
    (tools / "__init__.py").write_text("")
    (tools / "dispatcher.py").write_text(textwrap.dedent("""
        from toolr import command_group, Context
        from toolr.sources import DispatchCommand

        group = command_group("django", "Django")

        @group.command
        def django(ctx: Context, *, dispatched: DispatchCommand) -> int:
            print(f"auto:{dispatched.command}")
            return 0
    """))
    cmds = project / "apps" / "x" / "management" / "commands"
    cmds.mkdir(parents=True)
    (cmds / "migrate.py").write_text(
        "def add_arguments(self, parser):\n    parser.add_argument('--check', action='store_true')\n"
    )
    (tools / "pyproject.toml").write_text(textwrap.dedent("""
        [tool.toolr.argparse.django]
        scan_paths = ["apps/*/management/commands/*.py"]
        [[tool.toolr.argparse.django.attach]]
        parent = "django"
    """))

    # No explicit rebuild — the manifest should not exist yet.
    assert not (tools / ".toolr-manifest.json").exists()

    out = subprocess.run(
        ["toolr", "django", "migrate"],
        check=True, capture_output=True, text=True, cwd=project,
    ).stdout.strip()
    assert out == "auto:migrate"
    assert (tools / ".toolr-manifest.json").exists()
```

- [ ] **Step 2: Run**

```bash
uv run pytest tests/sources/test_e2e.py::test_e2e_auto_rebuild_runs_argparse -v
```

Expected: pass.

- [ ] **Step 3: Commit**

```bash
git add tests/sources/test_e2e.py
git commit -m "tests: e2e — auto-rebuild path runs the argparse scanner"
```

---

## Wrap-up

After Task 20:

- [ ] **Full suite check:**

```bash
uv run pytest -q
cargo test --workspace -- --test-threads=1
cargo clippy --workspace --tests -- -D warnings
uv run prek run --all-files
```

Expected: all green.

- [ ] **Open PR for Plan A:**

```bash
git-spice branch submit --draft \
  --title "feat(argparse): built-in argparse scanner + DispatchCommand dispatcher contract" \
  --body "Implements specs/2026-05-19-external-command-sources-design.md.

Adds:
- toolr.sources.{ArgSchema,CommandSchema,DispatchCommand} Python types.
- Annotation-driven dispatcher detection.
- Optional dispatched_from field on manifest Command.
- crates/toolr-core/src/argparse/ — config parser + AST scanner + grafter.
- Static-manifest integration so [tool.toolr.argparse.<name>] blocks are scanned on every rebuild + auto-rebuild.
- Runtime injection of DispatchCommand in the parent dispatcher.
- E2E tests covering explicit rebuild, multi-attach, collision, and auto-rebuild paths."
```

- [ ] **Update memory:** Note in `MEMORY.md` that the argparse scanner has landed and dispatchers must use `dispatched: DispatchCommand`.

---

## Out of scope (deferred)

- Python source-plugin contract (entry-point `scan()` callable, `SourceFragment`, freshness sums). Revisit when a non-AST-discoverable source (Jenkins, GitHub Actions, etc.) needs first-class support.
- Django `BaseCommand` MRO walking and `INSTALLED_APPS` discovery. Users declare `scan_paths` literally; framework knowledge is the user's responsibility for v1.
- Dynamic argparse parser builders (parsers built inside `if`/`for`/runtime callbacks).
- Multi-subparser files (`argparse.add_subparsers()`).
- Type inference beyond `type=int|float|str` literals.
- Help-text rendering polish for dispatched children (e.g. `(via argparse:django)` annotations).
