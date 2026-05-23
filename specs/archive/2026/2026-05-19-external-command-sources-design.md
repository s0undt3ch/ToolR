# External command sources via the argparse scanner

**Status:** Draft (2026-05-19, revised second pass)
**Topic:** Let toolr expose commands the user never wrote — by AST-scanning Python files that use `argparse` and grafting the discovered commands under user-written *dispatcher* toolr commands, with first-class completion and `--help`.

## Background

Toolr already supports three kinds of commands in its manifest:

1. **User-decorated commands** in `tools/*.py` (Rust AST-scanned by `parser::build_static_manifest`).
2. **Dynamic-only commands** discovered by importing user `tools.*` modules (the Python introspect helper at `_introspect.py`).
3. **Static fragments shipped by installed PyPI packages** (the `<venv>/lib/python*/site-packages/*/toolr-manifest.json` glob in `crates/toolr-core/src/third_party/glob.rs`).

The next problem on top of that: real users want to run *foreign* command sets through toolr — Django `manage.py` subcommands, ad-hoc argparse-based scripts, anything whose argument schema is statically discoverable from `parser.add_argument(...)` calls — without writing one toolr command per foreign command. Two driving cases:

1. **Django management commands.** Tracked at [#192](https://github.com/s0undt3ch/ToolR/issues/192). The killer feature is full Tab completion of `manage.py migrate --check`, `runserver --insecure`, etc., without Django runtime overhead. The vast majority of Django commands declare their args in plain `def add_arguments(self, parser): parser.add_argument(...)` calls — exactly the shape an AST scanner can read.

2. **Multiple dispatch surfaces over the same command set.** The same Django commands should be reachable both locally (`toolr django migrate --check`) and via a Jenkins job that runs `manage.py` on a worker with its own flags (`toolr jenkins job --cpu 5000m -- migrate --check`). The argument schema is discovered once; only the dispatcher behaviour differs.

Both want the same two things from toolr: **a stable way to graft externally-discovered commands into the manifest** (so completion and `--help` work natively), and **a user-pluggable dispatcher** (so the user decides what to actually do when one of those commands is invoked).

The choice in this spec is to deliver category-1 discovery via a **built-in Rust AST scanner** rather than a Python plugin contract. The scanner runs at `build_static_manifest` time, uses the `ruff_python_parser` crate already in toolr-core, requires no plugin install, and ships in the toolr binary. Sources whose schemas aren't AST-discoverable (Jenkins jobs, GitHub Actions workflows, etc.) are explicitly out of scope here — a plugin contract for them is the natural follow-up, but not part of v1 of this feature.

## Goal

Define (a) the user-side contract for declaring *dispatchers* (toolr commands annotated to receive a matched-child payload), and (b) the toolr-side argparse scanner that turns AST-discovered `parser.add_argument(...)` calls into manifest children grafted under those dispatchers.

In one picture:

```text
                     ┌────────────────────────────┐
   pyproject.toml ──►│ toolr-core static layer    │
   [tool.toolr.      │ - parses                   │
    argparse.<name>] │   [tool.toolr.argparse.*]  │
                     │ - AST-scans scan_paths     │
                     │   via ruff_python_parser   │
                     │ - extracts add_argument()  │
                     │ - applies common_args      │
                     │ - grafts children under    │
                     │   each [[attach]] parent   │
                     │   with dispatched_from set │
                     └────────────┬───────────────┘
                                  │ manifest.json
                                  ▼
                     ┌────────────────────────────┐
                     │ toolr runtime (Rust)       │
                     │ - clap nested subcommands  │
                     │ - completion / --help free │
                     │ - dispatcher detection     │
                     │   injects DispatchCommand  │
                     └────────────────────────────┘
```

## Non-goals

- **No Python plugin contract** in v1. Sources that need a Python callable (Jenkins API queries, framework-aware introspection beyond plain `add_argument`) are deferred. If/when a real case shows up, a plugin contract is a strict extension over this design — the user-side `DispatchCommand` API does not need to change.
- **No Django-specific logic.** No BaseCommand MRO walking, no `INSTALLED_APPS` discovery, no app-loading. Users declare `scan_paths` in `pyproject.toml`; toolr scans literally those files. Framework awareness is the user's job.
- **No runtime-only scanning.** All argparse discovery happens at `build_static_manifest` time. Completion is served by the static manifest, never re-runs the scanner.
- **No dynamic argparse parser builders.** Files where the parser is constructed inside `if`/`for`/runtime callbacks are out of scope; the scanner only sees lexically-direct `parser.add_argument(...)` calls (and the equivalents on the per-command argparse parser argument).

## Architecture

Three pieces, two of which extend existing code:

| Piece | Owner | New? |
|---|---|---|
| **Argparse AST scanner** — read `[tool.toolr.argparse.*]`, walk `scan_paths` with `ruff_python_parser`, extract `parser.add_argument` calls, produce `CommandSchema`s | `crates/toolr-core/src/argparse/` (new) | Yes |
| **Attachment + dispatcher** — `[tool.toolr.argparse.<name>]` + `[[attach]]` in `tools/pyproject.toml`, plus user-written dispatcher commands annotated with `dispatched: DispatchCommand` | User's project + `toolr.sources` Python module | Yes |
| **Manifest builder integration** — graft scanner output as children of declared parents, emit `dispatched_from` on each | Extends `toolr-core::parser::build_static_manifest` | Small additions |

### `pyproject.toml` configuration

In `tools/pyproject.toml`:

```toml
[tool.toolr.argparse.django]
scan_paths = ["apps/*/management/commands/*.py"]
# Hoisted once and applied to every command discovered by this block.
common_args = [
  { name = "verbosity", kind = "optional", default = "1",
    choices = ["0", "1", "2", "3"], help = "Output verbosity level" },
  { name = "traceback", kind = "flag",
    help = "Raise on CommandError exceptions" },
]

[[tool.toolr.argparse.django.attach]]
parent = "django"            # dotted name of a user-written dispatcher

[[tool.toolr.argparse.django.attach]]
parent = "jenkins.job"       # same scan results, second dispatcher
```

- The block key (`django` above) is a user-chosen identifier; appears in `dispatched_from` on each grafted child.
- `scan_paths` is a list of glob patterns relative to `tools_dir`'s parent (i.e., the project root). Resolved by the Rust scanner via the `glob` crate.
- `common_args` is optional; entries are applied to every discovered command after the per-command args.
- Each `[[attach]]` is one dispatcher routing. Multiple sources may attach to the same parent. Conflicts on child command name across sources sharing a parent are a build-time error.
- Per-attachment overrides (e.g. running the same source under two parents with different defaults) are not in v1 — drop the historical `extra = { ... }` idea.

### Dispatcher command (user-side)

```python
from toolr import command_group, Context
from toolr.sources import DispatchCommand

group = command_group("jenkins", title="Jenkins")

@group.command
def job(
    ctx: Context,
    *,
    cpu: str = "1000m",
    ram: str = "4Gi",
    log_level: str = "info",
    on_demand: bool = False,
    dispatched: DispatchCommand,   # any param name — annotation is the trigger
) -> int:
    """Submit a tools command to Jenkins."""
    return submit_jenkins(
        cpu=cpu, ram=ram, log_level=log_level, on_demand=on_demand,
        cmd=dispatched.command,
        args=dispatched.command_args,
    )
```

Canonical local dispatcher (used in docs):

```python
@group.command
def django(ctx: Context, *, dispatched: DispatchCommand) -> int:
    return ctx.run("python", "manage.py", *dispatched.argv).returncode
```

`ctx.run(...)` exists today (`crates/toolr-py/python/toolr/_context.py:195`).

### Detection rule

A command is treated as a dispatcher iff **exactly one** keyword-only parameter's annotation resolves to `toolr.sources.DispatchCommand`. Subclasses are not supported in v1. Multiple `DispatchCommand`-annotated params is a build-time error. Positional `DispatchCommand` parameter is a build-time error. The parameter name is free.

A command with a `DispatchCommand` parameter and no `[[attach]]` directing children at it is permitted (the dispatcher is just inactive — useful while iterating). The reverse — an `[[attach]]` targeting a command that lacks a `DispatchCommand` parameter — is a build-time error.

### `DispatchCommand`

Defined in `toolr.sources` (Python, msgspec):

```python
class DispatchCommand(Struct, frozen=True):
    command: str                          # e.g. "migrate"
    command_args: dict[str, Any]          # parsed kwargs from the child schema
    schema: CommandSchema                 # the matched child's schema

    @property
    def argv(self) -> list[str]:
        """Argparse-shaped argv reconstructed from command_args per schema."""
        ...
```

`CommandSchema` and `ArgSchema` are the same msgspec structs used internally by the Rust scanner to serialise discovered schemas into the manifest; they are public for `DispatchCommand.schema` consumers but their *primary* role is the internal Rust→Python wire format.

### Manifest field

`Command` gains one optional field:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub dispatched_from: Option<String>,
```

Set to the `[tool.toolr.argparse.<name>]` block key for every grafted child. `None` for every other command. **No `SCHEMA_VERSION` bump** — toolr is pre-1.0, the field is added to v1 in place and back-compat is handled by the `serde(default)` attribute.

### Scanner contract

The Rust argparse scanner (`crates/toolr-core/src/argparse/`):

1. Parses `[tool.toolr.argparse.*]` from `tools/pyproject.toml` (via the existing `toml` crate).
2. For each block, expands `scan_paths` globs against the project root.
3. For each file, parses with `ruff_python_parser` (already a workspace dep).
4. Walks the AST looking for calls of the form `<receiver>.add_argument(...)` where `<receiver>` is any identifier — typically `parser`, `subparser`, or `self.parser`. The scanner doesn't try to verify the receiver type; it just extracts every such call.
5. Per file, aggregates the calls into one `CommandSchema`:
   - `name` derived from filename: `migrate.py` → `migrate`. Underscores in the filename are preserved.
   - `summary`/`description` extracted from the module docstring (first paragraph / rest).
   - `arguments` = the extracted argparse args.
6. Applies `common_args` after per-file args (per-file wins on name collision).
7. Emits one child manifest `Command` per `CommandSchema`, set under each `[[attach]].parent`, with `dispatched_from = "<block-name>"`.

Argparse calls translated:

| `add_argument` signature | `ArgSchema.kind` | Notes |
|---|---|---|
| `add_argument('positional')` | `positional` | Single positional, required by default |
| `add_argument('--opt', default='x')` | `optional` | Value-taking option |
| `add_argument('--flag', action='store_true')` | `flag` | Boolean flag (also `store_false` — defaults inverted) |
| `add_argument('--many', action='append')` | `repeated` | Append-style repeated value |
| `add_argument(..., type=int)` | `type_annotation = "int"` | `type=int`/`float`/`str` extracted as a string; others dropped to `None` |
| `add_argument(..., choices=[...])` | `choices = [...]` | Literal lists/tuples of strings only |
| `add_argument(..., nargs=...)` | `nargs` | Passed through verbatim |

Anything the scanner can't statically resolve (dynamic `type=` callable, computed `choices`, etc.) is recorded as a warning and the corresponding field is left `None`. The command still appears in the manifest with a best-effort schema.

### Runtime dispatch path

In `crates/toolr/src/cli.rs`, when a matched leaf has `dispatched_from` set:

1. Parse the child's args normally via clap (no change to parsing).
2. Pack the parsed values into a Python dict, keyed by argument name.
3. Construct a `DispatchCommand` Python object: `{command, command_args, schema}` where `schema` is the child's `CommandSchema` serialised from the manifest.
4. When invoking the parent's Python function, fill the `DispatchCommand`-annotated parameter with that object. Other parent parameters (`cpu`, `ram`, …) are populated from clap as today, because clap parses both the parent's own flags and the child's subcommand naturally.

Completion, `--help`, error messages — all free. Children are first-class clap subcommands.

## Lifecycle

### Refresh triggers

- **Explicit:** `toolr project manifest rebuild` always re-runs the argparse scanner along with the rest of the static layer.
- **Implicit:** the existing auto-rebuild path (manifest missing, `dynamic_hash` empty, etc., see `crates/toolr/tests/cli_smoke.rs:206` and `crates/toolr-core/src/complete/freshness.rs`) calls into `build_static_manifest`, which runs the scanner automatically. First-run cost = one AST walk over `scan_paths`; subsequent runs short-circuit on file-mtime via the existing static-layer hash.

The freshness story is therefore **identical to existing static-layer freshness** — no separate per-source cache file; the manifest itself is the cache, and the static-layer rebuild detection (file mtime over `tools/` + `scan_paths`) drives invalidation.

### Build-time validation (the hard-fail set)

| Condition | Message references |
|---|---|
| `[[attach]]` `parent` doesn't resolve to a known command | attach line + closest existing dotted name |
| Resolved parent has no `DispatchCommand`-annotated keyword-only param | function file:line + actual signature |
| Two sources attached to the same parent produce the same child name | both source names + colliding child name |
| `DispatchCommand`-annotated param not keyword-only | function file:line + parameter name |
| Multiple `DispatchCommand` params on the same command | function file:line + parameter names |
| `scan_paths` matches no files | warning only (not a hard fail) |
| File can't be parsed | warning only, file skipped |
| `add_argument` call uses an unresolvable `type=`/`choices=` | warning, field left `None` |

### Testing strategy

- **Detection rule** — table-driven tests on `detect_dispatch_parameter` in the existing `_signature.py` test pattern.
- **Schema types** — msgspec round-trip + `DispatchCommand.argv` reconstruction over the matrix of `kind`s.
- **Scanner (Rust)** — golden tests over fixture `.py` files in `crates/toolr-core/src/argparse/fixtures/`. Each fixture is one `.py` plus an expected `CommandSchema` JSON.
- **Grafting** — Rust unit tests that feed synthetic `CommandSchema`s through the attach + collision pipeline.
- **E2E** — `tests/argparse/` suite: build a tiny project with `tools/`, `tools/pyproject.toml`, `apps/x/management/commands/migrate.py`. Run `toolr project manifest rebuild`, then `toolr django migrate --check`. Cover the auto-rebuild path explicitly (delete the manifest, run the command, assert it was rebuilt).

## Migration notes

For the dashtastic Jenkins use case: the Django half can be done today with the argparse scanner. The Jenkins half (jobs discovered via HTTP API) is out of scope for this feature and waits for the deferred plugin contract.

For users who currently maintain hand-written toolr commands that mirror Django `manage.py` commands: install nothing, drop the hand-written commands, add the `[tool.toolr.argparse.django]` block, run `toolr project manifest rebuild`. Done.

## Alternatives considered

### Python plugin contract (`toolr.sources` entry points with `scan()` callable)

Considered at length earlier in this design. The plugin model would have let third-party packages contribute schemas through a Python-callable `scan(root, config) -> SourceFragment` entry point. Rejected for v1 because:

1. The most-asked-for case (Django `add_arguments`) is statically AST-discoverable; a Rust scanner inside the static layer is faster, simpler, and requires no plugin install.
2. The Jenkins case that *did* need a plugin contract turned out to be a category we don't yet need to ship (dashtastic continues to use its existing `tools/jenkins/cli.py` until we decide otherwise).
3. Reducing the v1 surface to "Rust scanner + DispatchCommand contract" makes the user-side API a strict subset of what a future plugin contract would also expose — the plugin path becomes a strict extension, not a competing design.

The plugin contract remains a sensible follow-up when (and if) a non-argparse-discoverable source needs first-class toolr support.

### Flat: outer flags merged into every generated command

Considered. Would have produced `toolr django migrate --cpu 5000m --check` with the parent's flags duplicated into every Django child. Pros: flatter UX; no `--` separator. Cons: M outer × N children explosion in the manifest, name conflicts when parent and child both define `--verbosity`, harder for the user to keep the dispatcher's "outer concerns" mentally separate from each Django command's "inner concerns". Rejected in favour of the hierarchical clap-nested-subcommand form.

### Source plugins emit parent-aware fragments directly

Considered and rejected — would have required plugins to read `pyproject.toml` and resolve parents themselves. The chosen design keeps that logic in toolr core; sources stay focused on discovery.

### Reference plugin (`toolr-django`) shipped in-tree

Considered through several rounds. The argparse-scanner choice obsoletes the need for a Django-specific plugin entirely — Django commands fall under the generic argparse scanner. If a future Django need arises that the generic scanner can't satisfy (e.g. `BaseCommand` MRO walking), it would come back as a plugin under the deferred contract.

## Open questions

- **Command name derivation.** Filename-stem only (`migrate.py` → `migrate`), or also support a `name = "…"` directive inside the file (e.g. a module docstring tag)? v1 suggestion: filename only; revisit when a real conflict appears.
- **Multi-command files.** Some argparse scripts define multiple subcommands inside one file via `argparse.add_subparsers()`. v1 suggestion: treat each subparser's calls as one `CommandSchema` keyed on the subparser name; if none, fall back to filename.
- **Type inference beyond `type=int|float|str`.** Pydantic, attrs, custom callables, etc. v1 suggestion: leave `type_annotation = None` (manifest still completes; the value is parsed as a string at dispatch).
- **`scan_paths` glob semantics.** Use the `glob` crate's default (no recursive `**` without trailing `/`)? Or always-recursive? v1 suggestion: support `**` recursive globs (`apps/*/management/commands/**/*.py`) since Django apps may nest commands under subdirectories.
- **Help-text rendering for dispatched commands.** When the user runs `toolr jenkins job --help`, should the rendered output annotate `(via argparse:django)` next to each child? Cosmetic; not load-bearing for v1.
