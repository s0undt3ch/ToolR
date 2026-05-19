# External command sources and dispatchers

**Status:** Draft (2026-05-19)
**Topic:** Let toolr expose commands that the user never wrote — discovered by external source plugins (e.g. Django management commands, Jenkins jobs) — under user-declared dispatcher commands, with first-class completion and `--help`.

## Background

Toolr's manifest model already supports static third-party fragments (Plan 5). The next problem on top of that: real users want to run *foreign* command sets through toolr — Django `manage.py` subcommands, Jenkins jobs, in principle anything with a discoverable argument schema — without writing one toolr command per foreign command. Two concrete cases drive this:

1. **Django management commands.** Issue [#192](https://github.com/s0undt3ch/ToolR/issues/192) sketches a `toolr-django` plugin that AST-scans `*/management/commands/*.py`, extracts each command's `add_arguments`, and exposes them under a `django` group. Killer feature: full Tab completion of `manage.py migrate --check`, `runserver --insecure`, etc., without importing Django at completion time.

2. **Jenkins jobs.** The dashtastic repo's `tools/jenkins/cli.py` queries the Jenkins API at startup, caches results in sqlite, and dynamically builds an argparse tree — one subparser per Jenkins job, each with its job-specific parameter set. Same shape as Django: a foreign source provides a set of "commands" with structured args.

Both want the same thing from toolr: **a stable, parent-agnostic way to graft externally-discovered commands into the manifest** so that completion and `--help` work natively, and **a user-pluggable dispatcher** that decides how to actually run each match (locally? submit to Jenkins? something else?).

Crucially, the same Django source set should be reachable via **multiple dispatchers** on the same user's machine — locally (`toolr django migrate --check`) and via a Jenkins job that runs `manage.py` on a worker (`toolr jenkins job --cpu 5000m -- migrate --check`). The source-side scan happens once; only the dispatch side differs.

## Goal

Define the contract by which an external Python package (a *source plugin*) feeds toolr's manifest with command schemas it discovered, and the user-side surface by which those commands are routed through a *dispatcher* toolr command of the user's own design.

In one picture:

```text
                     ┌────────────────────────────┐
                     │ Source plugin              │
                     │  toolr-django (pyo3 wheel) │
                     │  crates/toolr-django/      │
                     │  scan() -> SourceFragment  │
                     └────────────┬───────────────┘
                                  │ parent-agnostic schema
                                  ▼
                     ┌────────────────────────────┐
   pyproject.toml ──►│ toolr manifest builder     │
   [[attach]]        │ - resolves entry points    │
                     │ - calls scan() w/ freshness│
                     │ - grafts children under    │
                     │   each declared parent     │
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

- This spec defines the toolr-side contract and the manifest-builder integration, not the internals of the reference plugin. `toolr-django` lives in-tree (see Alternatives considered) but its scanner details — `add_arguments` AST extraction, `BaseCommand` MRO handling, app discovery — are deferred to the implementation plan and may evolve separately from the contract.
- Not designing a Jenkins-side source plugin. Same reasoning; the dashtastic migration is a downstream consumer that consumes whatever contract this spec lands.
- No new clap escape hatch (the proxy/raw-bucket variadic idea from the brainstorm origin is unnecessary for this design; documented in "Alternatives considered" for future reference).
- No dynamic / runtime-only scanning. Sources are scanned at `toolr build` time only; the runtime never imports a source plugin.
- No daemon, no background refresh, no filesystem watcher.

## Architecture

Three concepts, two of which already exist:

| Piece | Owner | New? |
|---|---|---|
| **Source plugin** — produces parent-agnostic `SourceFragment` schemas | In-tree workspace member (`crates/toolr-django/`), independent PyPI release | Yes (contract); reuses Plan 5 manifest-fragment ideas |
| **Attachment + dispatcher** — `[tool.toolr.sources.<name>]` in `tools/pyproject.toml`, plus a user-written dispatcher command with a `DispatchCommand`-annotated parameter | User's project | Yes |
| **Manifest builder** — reads sources, calls plugins, grafts children under declared parents | toolr (`toolr build`) | Small additions to existing builder |

### Source plugin contract

A source plugin is a regular PyPI package. It declares a Python entry point under `toolr.sources` whose value is a callable named `scan`:

```toml
# Source plugin's own pyproject.toml
[project.entry-points."toolr.sources"]
django = "toolr_django.source:scan"
```

The callable's signature:

```python
def scan(*, root: Path, config: dict[str, Any]) -> SourceFragment: ...
```

- `root` — the user's project root (containing `tools/pyproject.toml`).
- `config` — the user's `[tool.toolr.sources.<name>]` table parsed into a dict. Opaque to toolr; whatever keys the plugin documents.
- Returns a `SourceFragment` (defined in `toolr.sources`).

The plugin **does not know which toolr group its output will be attached under**. It returns a parent-agnostic schema; attachment is the manifest builder's job (see "User surface" below).

`scan` is synchronous. Plugins that need to do network I/O (Jenkins API) do it inline; toolr won't drive an event loop on their behalf.

#### `SourceFragment` shape

Defined in `toolr.sources` as msgspec Structs (frozen):

```python
class SourceFragment(Struct, frozen=True):
    schema_version: int                  # currently 1
    common_args: list[ArgSchema]         # hoisted once, applied to every command at attach time
    commands: list[CommandSchema]
    freshness: FileSetFreshness | OpaqueFreshness

class CommandSchema(Struct, frozen=True):
    name: str                            # e.g. "migrate"
    summary: str                         # one-liner
    description: str                     # full help body (may be empty)
    arguments: list[ArgSchema]           # command-specific args (no common_args here)

class ArgSchema(Struct, frozen=True):
    name: str
    kind: Literal["positional", "optional", "flag", "repeated"]
    help: str
    default: str | None
    choices: list[str] | None
    metavar: str | None
    type_annotation: str | None          # "str" / "int" / "float" / "bool"
    nargs: Literal["*", "+", "?"] | int | None

class FileSetFreshness(Struct, frozen=True, tag="files"):
    files: list[Path]                    # absolute, resolved against root

class OpaqueFreshness(Struct, frozen=True, tag="opaque"):
    blob: bytes                          # plugin-defined, stored verbatim by toolr
```

`common_args` is the BaseCommand-style "every Django command takes `--verbosity`, `--traceback`" set. The manifest builder spreads them across every attached child so they don't have to be duplicated in every `CommandSchema`.

`freshness` is a typed sum so the cache file stays self-describing:

- **`FileSetFreshness`** — toolr stat()s every path on subsequent builds; mismatched mtime/size means re-scan. Django plugin returns this with the discovered command files.
- **`OpaqueFreshness`** — plugin owns the check via an optional sibling entry point:

  ```toml
  [project.entry-points."toolr.sources"]
  jenkins = "toolr_jenkins.source:scan"
  jenkins_is_fresh = "toolr_jenkins.source:is_fresh"
  ```

  with signature `def is_fresh(*, blob: bytes, root: Path, config: dict) -> bool`. If no `<name>_is_fresh` entry point is registered, toolr treats opaque freshness as always stale.

### User surface

#### Attachment config

In the user's `tools/pyproject.toml`:

```toml
[tool.toolr.sources.django]
# Everything here is opaque to toolr — passed through to the plugin's
# `scan(..., config=...)` argument as-is.
manage_py  = "src/manage.py"
scan_paths = ["apps/*/management/commands"]

# One [[attach]] per dispatcher routing.
[[tool.toolr.sources.django.attach]]
parent = "django"           # dotted name of the dispatcher command

[[tool.toolr.sources.django.attach]]
parent = "jenkins.job"      # same source, second dispatcher
```

The table key `django` matches the entry-point name (`[project.entry-points."toolr.sources"] django = ...`). No magic strings beyond that.

Multiple sources may attach to the same parent. Conflicts on child command name across sources sharing a parent are a build-time error.

#### Dispatcher command

The user writes the dispatcher as a normal toolr command. The signal that *this* command is a dispatcher (and not a leaf the user wants to invoke directly) is the presence of a parameter annotated with `DispatchCommand`:

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
    dispatched: DispatchCommand,      # any param name — annotation is the trigger
) -> int:
    """Submit a tools command to Jenkins."""
    return submit_jenkins(
        cpu=cpu, ram=ram, log_level=log_level, on_demand=on_demand,
        cmd=dispatched.command,
        args=dispatched.command_args,
    )
```

`DispatchCommand` lives in `toolr.sources`:

```python
class DispatchCommand(Struct, frozen=True):
    command: str                          # e.g. "migrate"
    command_args: dict[str, Any]          # parsed kwargs from the child schema
    schema: CommandSchema                 # the matched child's schema

    @property
    def argv(self) -> list[str]:
        """Argparse-shaped argv reconstructed from command_args per schema.

        Walks schema.arguments + the inherited common_args, emits each value
        as one or more argv tokens (flag, --opt value, repeated --opt value
        per list element, omits any value equal to the schema default)."""
        ...
```

Canonical local-dispatcher example, used in docs:

```python
@group.command
def django(ctx: Context, *, dispatched: DispatchCommand) -> int:
    return ctx.run("python", "manage.py", *dispatched.argv).returncode
```

`ctx.run(...)` already exists (`toolr-py` `_context.py:195`). No new runtime API needed for the local case.

#### Detection rule

A command is treated as a dispatcher iff **exactly one** keyword-only parameter's annotation resolves to `toolr.sources.DispatchCommand` (the class itself; subclasses are not supported in v1). The parameter name is free. Position must be keyword-only. Multiple `DispatchCommand`-annotated parameters on the same command is a build-time error.

A `DispatchCommand`-annotated parameter on a command that has no `[[attach]]` directing children at it is permitted (the dispatcher is just inactive). The reverse — an `[[attach]]` targeting a command that lacks a `DispatchCommand` parameter — is a build-time error.

### Manifest builder integration

Three new phases in `toolr build`, sequenced between "collect Python registry" and "write manifest":

#### 1. Resolve sources

For each `[tool.toolr.sources.<name>]` block:

1. Look up the matching entry point under `toolr.sources` in the installed tools venv. Missing → hard fail with a suggested `pip install` line that names the convention (`toolr-<name>` if applicable).
2. Import the `scan` callable. Import error → hard fail naming the entry point target.
3. For each `[[attach]]` entry, validate that `parent` resolves to a known command in the in-progress manifest (i.e., the user's Python decorators have been imported and registered). Missing → hard fail naming both the attach line and the closest existing dotted name.
4. For each named parent, validate it has at least one `DispatchCommand`-annotated keyword-only param. Missing → hard fail naming the function file:line and the existing signature.

#### 2. Run scans (freshness short-circuit)

```text
for source in sources:
    cached = freshness_cache.get(source.name)
    if cached:
        if isinstance(cached.freshness, FileSetFreshness):
            if all paths stat()-match cached, reuse cached.fragment
        elif isinstance(cached.freshness, OpaqueFreshness):
            if <name>_is_fresh entry point exists and returns True, reuse
    fragment = plugin.scan(root=root, config=config)
    freshness_cache.put(source.name, fragment)
```

Cache lives at `tools/.toolr/sources/<name>.json`. Same gitignore convention as the existing manifest cache.

#### 3. Graft fragments under parents

For each `[[attach]]` entry `parent = "<dotted>"`:

1. Locate the parent command node.
2. For each `CommandSchema` in the fragment:
   - Build a child `Command` manifest entry:
        - `name` = `schema.name`
        - `arguments` = `schema.arguments` + the fragment's `common_args` (in that order; child wins on name collisions).
        - `summary` = `schema.summary`, `description` = `schema.description`.
   - Set the child's `target` to the parent's `target` (same Python function will be invoked).
   - Mark the child with `dispatched_from = "<source-name>"` so the runtime knows to inject `DispatchCommand` rather than the parent's normal keyword args.
3. Reject name collisions across sources attaching to the same parent.

The manifest format gains one optional field on `Command`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub dispatched_from: Option<String>,
```

Bump `FRAGMENT_SCHEMA_VERSION` (the **existing** manifest-fragment schema in `toolr-core`) from 1 → 2. Older toolrs reading a v2 manifest fail at load time with a clear "rebuild required" error.

Note that this is distinct from `SourceFragment.schema_version` defined above. The two schemas evolve independently:

- `FRAGMENT_SCHEMA_VERSION` — the on-disk manifest format consumed by the toolr runtime. Bumped here for `dispatched_from`.
- `SourceFragment.schema_version` — the plugin↔toolr contract for what `scan()` returns. Starts at 1; bumped only when the source-plugin contract itself changes.

### Runtime dispatch path

In `crates/toolr/src/cli.rs`, when a matched leaf has `dispatched_from` set:

1. Parse the child's args normally via clap (no change to parsing).
2. Pack the parsed values into a Python dict, keyed by argument name.
3. Construct a `DispatchCommand` Python object: `{command, command_args, schema}` where `schema` is the child's `CommandSchema` (serialised into the manifest fragment).
4. When invoking the parent's Python function, fill the `DispatchCommand`-annotated parameter with that object. Other parent parameters (`cpu`, `ram`, …) are populated from clap as today, because clap parses both the parent's own flags and the child's subcommand naturally.

Completion, `--help`, error messages — all free. Children are first-class clap subcommands.

## Lifecycle

### Refresh triggers

- **Explicit**: `toolr build` always picks up source changes (subject to freshness short-circuit).
- **Implicit**: `toolr build --check` mode (exits non-zero if any source is stale or the manifest is out of date), suitable for pre-commit / CI.
- Source plugins ship their own pre-commit hooks if they want commit-time freshness (`toolr-django` would filter on `^manage\.py$|.*/management/commands/.*\.py$`).

### Build-time validation (the hard-fail set)

| Condition | Message references |
|---|---|
| Source declared in pyproject.toml but no matching `toolr.sources` entry point installed | source name + suggested install command |
| `[[attach]]` `parent` doesn't resolve to a known command | attach line + closest existing dotted name |
| Resolved parent has no `DispatchCommand`-annotated keyword-only param | function file:line + actual signature |
| Two sources attached to the same parent produce the same child name | both source names + colliding child name |
| `DispatchCommand`-annotated param not keyword-only | function file:line + parameter name |
| Manifest fragment schema_version mismatch | plugin name + version + supported range |

Every message names both ends of the broken link.

### Testing strategy

- **Schema types** (`toolr.sources`): msgspec round-trip tests, `DispatchCommand.argv` table-driven over positional / flag / repeated / optional-with-default / suppress-default cases.
- **Manifest grafting** (Rust): unit tests over synthetic `SourceFragment` + synthetic registry, asserting children land under the right parents with `dispatched_from` set. Cover the hard-fail matrix.
- **Runtime injection** (Rust → pyo3): integration tests via a fake in-tree source plugin (`tests/support/toolr-source-fake/`) that emits a fixed fragment. Exercise `toolr build` then `toolr <parent> <child> --foo bar` and assert the dispatcher function received the right `DispatchCommand`.
- **Entry-point resolution**: dedicated tests for the "plugin not installed" / "import error" / "wrong return type" failure modes.

## Migration notes

For the dashtastic Jenkins setup (the design's other driver), the current `tools/jenkins/cli.py` becomes two pieces:

1. A `toolr-jenkins-jobs` source plugin (separate repo on PyPI). Reuses dashtastic's existing sqlite-cached Jenkins API query code. Emits `SourceFragment` with `OpaqueFreshness` keyed on the API ETag.
2. A `jenkins.job` dispatcher command (lives in dashtastic's own `tools/`) with the `dispatched: DispatchCommand` parameter and the existing Jenkins-submission logic in its body.

The runtime parser-building inside `job_to_parser` (current `tools/jenkins/cli.py:58-100`) goes away entirely; clap handles the same job tree via the manifest.

The Django case is symmetric: `pip install toolr-django`, add `[tool.toolr.sources.django]`, write a 5-line `@django.command` dispatcher that calls `ctx.run("python", "manage.py", *dispatched.argv)`.

## Alternatives considered

### Proxy / raw-bucket variadic decorator

The original framing of this design space — "let a `*args: str` variadic swallow `--flags` so the user can implement an in-Python proxy command" — solves a strictly weaker problem. It produces a runtime parser with one bucket per dispatcher; completion of inner commands is impossible because their schemas are never seen. The source-plugin design replaces it for the cases we have, but the variadic primitive remains a reasonable future addition for non-introspectable sources (e.g. proxying to an arbitrary binary whose CLI surface can't be discovered). Tracked as a future feature, not part of this spec.

### Flat: outer flags merged into every generated command

Considered and rejected. Would have produced `toolr django migrate --cpu 5000m --check` with the parent's flags duplicated into every Django child. Pros: flatter UX; no `--` separator. Cons: M outer × N children explosion in the manifest, name conflicts when parent and child both define `--verbosity`, harder for the user to keep the dispatcher's "outer concerns" mentally separate from each Django command's "inner concerns".

### Source plugins emit parent-aware fragments directly

Considered and rejected. Would have meant `toolr-django` reads the user's `pyproject.toml`, knows about the user's parent commands, and produces one pre-attached fragment per attachment. Each plugin would re-implement TOML parsing + parent resolution. The chosen design keeps that logic in toolr core; plugins stay focused on discovery.

### Executable plugin contract (CLI binary emitting JSON)

Considered. Would have decoupled plugins from Python. Rejected for v1: the marginal cost of supporting non-Python plugins is high (process spawn per build, JSON envelope versioning, harder error reporting) and there's no signal yet that non-Python plugins are wanted. Revisit if a Go/Rust-only plugin author shows up.

### Reference plugin shipped out of tree from day one

Considered and reversed during the brainstorm. Out-of-tree would have forced the contract to be honest — anything the plugin needs would have to be reachable through public toolr APIs, with no monorepo shortcuts. Reversed because the contract is still moving and cross-repo schema-bump coordination is friction we don't need yet. **Chosen approach:** the reference `toolr-django` lives in-tree at `crates/toolr-django/` as an additional Rust + pyo3 workspace member. It participates in the *existing* CI (lint / typecheck / test / wheel-build matrix in `ci.yml` + `_test.yml` + `_build.yml`) on every PR alongside the rest of the workspace, so schema changes on one side always exercise the other. **Only the release path is separate:** a new `release-toolr-django.yml` cuts `toolr-django X.Y.Z` independently of `toolr A.B.C`, so the two PyPI versions move at their own cadences. Anything the plugin needs from toolr still imports through `toolr.sources` — no direct crate-internal dependencies, keeping the contract honest even with in-tree proximity.

## Open questions

- **Schema versioning across the toolr ↔ plugin boundary.** Plugins declare a `schema_version` on their `SourceFragment`. With the in-tree-plus-separate-release decision, the toolr↔toolr-django boundary becomes lockstep (both bump in the same PR), so strict-equality enforcement is unnecessary inside this repo. The version field still matters for *future* third-party plugins that take a dependency on whichever toolr version they were built against. Default suggestion: emit a clear `toolr build` warning on `cached.schema_version < current` (suggest plugin re-install), hard fail on `cached.schema_version > current` (toolr can't read it).
- **`scan_paths` semantics.** Plugins decide whether `scan_paths` is a glob, regex, or whatever — toolr passes it through. Should toolr offer any helpers (e.g. a `toolr.sources.glob_paths(root, patterns)` utility)? Open; YAGNI for v1, add when a second plugin needs the same logic.
- **Performance budget for `scan()` at build time.** No hard limit proposed. Plugins are expected to use their `freshness` callback to skip work — toolr just times the calls and emits a warning if any one source takes >5s on a fresh build.
- **Help text rendering for dispatched commands.** When the user runs `toolr jenkins job --help`, the rendered output should make it clear that `migrate`, `runserver`, etc. are dispatched-from-source children, not direct subcommands the user wrote. Cosmetic only; suggested approach is a `(via toolr-django)` annotation in the children listing. Not load-bearing for v1.
