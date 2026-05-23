# Filling the gaps from Plan A

**Status:** Draft (2026-05-19)
**Topic:** Two follow-up fixes deferred from `specs/2026-05-19-external-command-sources-plan-a.md`:
auto-rebuild on missing manifest, and routing the dispatcher's own CLI flags
through the grafted-child invocation path.

## Background

Plan A's PR (#222) shipped the built-in argparse scanner and the
`DispatchCommand` runtime contract, but two known gaps were documented as
deferred work:

1. **Auto-rebuild on missing manifest.** The user originally said *"if the
   manifest file was missing, it would be auto-generated (yeah, slower UX for
   one time)."* That intent isn't implemented today: `main.rs::load_or_empty`
   silently returns an empty manifest when `tools/.toolr-manifest.json` is
   absent, so clap rejects every user command before any rebuild can fire.

2. **Dispatcher's own CLI flags reachable on the child invocation path.** The
   external-command-sources spec called for invocations like
   `toolr jenkins job --cpu 5000m --ram 12Gi migrate --check`. Today, grafted
   children are *siblings* of the dispatcher inside the parent group, so the
   reachable form is `toolr jenkins migrate ...` and the dispatcher's outer
   flags have nowhere to live on that path.

Both gaps are bounded and independent. This spec fixes both in one PR with
one implementation plan.

## Goal

After this work lands:

- `toolr <user-cmd>` on a fresh clone (no manifest yet) succeeds — toolr
  detects the missing manifest, runs a full rebuild, and continues into the
  command. Tab completion and built-ins (`--help`, `project`, `self`, `init`)
  skip the auto-rebuild.
- `toolr <group> <dispatcher> --outer-flags... <child> --child-flags...` works
  end-to-end. The dispatcher receives both its outer kwargs and a
  `DispatchCommand` payload for the child.

## Non-goals

- No widening of `invoke_dispatcher` semantics. The dispatcher still requires
  a `dispatched: DispatchCommand` kwarg; bare `toolr <group> <dispatcher>`
  (no child) is rejected by clap at parse time via `subcommand_required(true)`.
- No new auto-rebuild trigger beyond missing-manifest. The existing
  `ensure_dynamic_layer_fresh` path (stale dynamic hash) is unchanged.
- No support for an "is-dispatcher" hint on the **dynamic** registry side. The
  flag is set by the **static** argparse scanner at graft time.

## Architecture

Two independent fixes, sharing one PR:

```text
                     ┌────────────────────────────┐
   Gap 1 fix         │ main.rs::ensure_manifest_  │
   (manifest         │   present_or_bootstrap     │
    bootstrap)       │ - argv first-pos inspect   │
                     │ - tools/pyproject.toml?    │
                     │ - manifest exists?         │
                     │ - if all green for rebuild:│
                     │   eprintln + call          │
                     │   rebuild_manifest_full    │
                     └──────────────┬─────────────┘
                                    │ load_manifest
                                    ▼
                     ┌────────────────────────────┐
   Gap 2 fix         │ cli.rs::build_group_subtree│
   (CLI tree shape   │ - read is_dispatcher       │
    for dispatchers) │ - when true: dispatcher    │
                     │   becomes Command with     │
                     │   its own args + grafted   │
                     │   children as subcommands  │
                     │ - children no longer       │
                     │   sibling-attach to group  │
                     └──────────────┬─────────────┘
                                    │ clap parse
                                    ▼
                     ┌────────────────────────────┐
                     │ dispatch.rs                │
                     │ - same walk, but           │
                     │   group_full_path needs    │
                     │   tweak for 3-segment      │
                     │   path (group/dispatcher/  │
                     │   child)                   │
                     └────────────────────────────┘
```

| Gap | Files | New manifest field |
|---|---|---|
| 1 — auto-rebuild | `crates/toolr/src/main.rs`, possibly extracted to `crates/toolr/src/bootstrap.rs` | None |
| 2 — dispatcher hosts children | `crates/toolr-core/src/manifest/model.rs`, `crates/toolr-core/src/argparse/{attach,mod}.rs`, `crates/toolr-core/src/parser/build.rs`, `crates/toolr/src/cli.rs`, `crates/toolr/src/dispatch.rs` | `is_dispatcher: bool` on `Command` |

Unchanged: the `DispatchCommand` Python contract, the argparse AST scanner,
the JSON-spec dispatch wire format, the Python runner's `invoke_dispatcher`.
Children's `group` field still equals `attachment.parent` — the new tree
shape is purely a CLI-build concern, not a manifest-hierarchy change.

## Gap 1 — Auto-rebuild on missing manifest

### Decision logic

`ensure_manifest_present_or_bootstrap(cwd, argv) -> Result<()>`:

1. Discover project root. If none, return `Ok` (not a toolr project).
2. If `tools/pyproject.toml` is missing, return `Ok` (toolr-init not yet run).
3. If `tools/.toolr-manifest.json` exists, return `Ok` (existing freshness path handles it).
4. Cheap argv inspection: if `should_skip_auto_rebuild(argv)` returns true, return `Ok`.
5. Resolve the tools venv (`toolr_core::venv::resolve_venv_path`).
6. If the venv's python doesn't exist, return `Ok` — let the normal downstream error surface (same fallback `ensure_dynamic_layer_fresh` uses).
7. Print `toolr: manifest missing; building (first-time setup)...` to stderr.
8. Call `toolr_core::dynamic::rebuild_manifest_full(&root, &python, &venv_dir)`.
9. On error: propagate. `main.rs` prints it and exits 2. Don't fall through to empty manifest.
10. Return `Ok`. `main.rs::load_or_empty` will now find the manifest.

### Argv inspector

```rust
fn should_skip_auto_rebuild(argv: &[String]) -> bool {
    const BUILTINS: &[&str] = &["__complete", "project", "self", "init"];
    const HELP_FLAGS: &[&str] = &["--help", "--version", "-h", "-V"];

    if argv.iter().skip(1).any(|a| HELP_FLAGS.contains(&a.as_str())) {
        return true;
    }
    let first_positional = argv.iter().skip(1).find(|a| !a.starts_with('-'));
    match first_positional {
        None => true,
        Some(name) => BUILTINS.contains(&name.as_str()),
    }
}
```

Properties:

- `toolr <user-cmd>` (any user-defined command) → rebuilds.
- `toolr --debug <user-cmd>` (leading global flag) → rebuilds.
- `toolr --help` / `-h` / `--version` / `-V` → skips.
- `toolr` alone → skips (will print group help anyway).
- `toolr __complete ...` → skips (tab completion must be fast).
- `toolr project manifest rebuild` / `toolr self cache list` / `toolr init` → skips.

### Tests

Table-driven `should_skip_auto_rebuild` tests (10 cases listed under "Test
surface" below).

End-to-end: unskip the existing
`test_e2e_auto_rebuild_runs_argparse` in `tests/sources/test_e2e.py`. The
test already deletes the manifest and invokes `toolr django migrate --check`,
asserting the manifest is recreated and the dispatcher receives the right
payload.

### UX message

`toolr: manifest missing; building (first-time setup)...` — matches the
existing `toolr: dynamic manifest layer stale; regenerating...` tone.

## Gap 2 — Dispatcher hosts grafted children

### Manifest field

`crates/toolr-core/src/manifest/model.rs`:

```rust
pub struct Command {
    // ... existing fields ...

    /// True when this command hosts grafted children as its own
    /// subcommands. Set by `argparse::run_for_project` on the parent
    /// dispatcher entry whenever a `[[tool.toolr.argparse.*.attach]]`
    /// directs children at it. Read by the CLI builder to decide
    /// whether to build the command as a flat leaf or as a parent
    /// that owns children.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_dispatcher: bool,
}
```

Same `serde(default, skip_serializing_if)` pattern as `dispatched_from`. No
`SCHEMA_VERSION` bump (pre-1.0). Every existing `Command { ... }` literal
gets `is_dispatcher: false,` added.

### Setting the flag at graft time

`argparse::run_for_project` is extended to return:

```rust
pub struct GraftResult {
    /// {parent_dotted_name -> Vec<grafted child Command>}.
    pub children_by_parent: HashMap<String, Vec<Command>>,
    /// Dotted names of parents that received at least one grafted child.
    pub dispatchers: HashSet<String>,
}
```

`build_static_manifest_inner` consumes both: appends children as today, then
walks `manifest.commands.iter_mut()` and flips `is_dispatcher = true` on every
entry whose dotted name is in `grafted.dispatchers`.

The dotted-name derivation is the same one already used to populate the
`parents` map in `build_static_manifest_inner`; factor it into a shared
`fn dotted_name(cmd: &Command) -> String` helper to keep the two call sites
in lockstep.

### CLI tree reshape

`crates/toolr/src/cli.rs::build_group_subtree`:

```rust
fn build_group_subtree(
    group: &Group,
    manifest: &Manifest,
    children: &HashMap<Option<String>, Vec<&Group>>,
) -> Command {
    let full_path = group.full_path();
    let mut g = Command::new(group.name.clone()).about(group.title.clone());
    if !group.description.is_empty() {
        g = g.long_about(group.description.clone());
    }

    // Bucket grafted children of this group by their dispatcher's
    // (module, function). `graft_children` copies those from the parent
    // dispatcher entry, so the pair is a precise dispatcher identity.
    let grafted_by_dispatcher: HashMap<(&str, &str), Vec<&Command>> = manifest
        .commands
        .iter()
        .filter(|c| c.group == full_path && c.dispatched_from.is_some())
        .fold(HashMap::new(), |mut acc, c| {
            acc.entry((c.module.as_str(), c.function.as_str()))
                .or_default()
                .push(c);
            acc
        });

    // For each NON-grafted command in this group, decide whether to
    // build it as a dispatcher (with its args + the bucket as
    // subcommands) or a normal leaf.
    for cmd in manifest
        .commands
        .iter()
        .filter(|c| c.group == full_path && c.dispatched_from.is_none())
    {
        if cmd.is_dispatcher {
            let children = grafted_by_dispatcher
                .get(&(cmd.module.as_str(), cmd.function.as_str()))
                .cloned()
                .unwrap_or_default();
            g = g.subcommand(build_dispatcher_command(cmd, &children));
        } else {
            g = g.subcommand(build_user_command(cmd));
        }
    }

    if let Some(child_groups) = children.get(&Some(full_path)) {
        for child in child_groups {
            g = g.subcommand(build_group_subtree(child, manifest, children));
        }
    }
    g
}

fn build_dispatcher_command(dispatcher: &Command, children: &[&Command]) -> Command {
    let mut c = build_user_command(dispatcher).subcommand_required(true);
    for child in children {
        c = c.subcommand(build_user_command(child));
    }
    c
}
```

Grafted children no longer appear as direct subcommands of the group. They
appear as subcommands of the dispatcher. A dispatcher's `subcommand_required(true)`
ensures bare `toolr <group> <dispatcher>` invocations get a clear "Usage" message
from clap rather than a runtime "missing dispatched argument" error.

### Dispatch path lookup

`crates/toolr/src/dispatch.rs` currently computes
`group_full_path = path[..len-1].join(".")`. For a dispatched leaf the path
is three segments (`["jenkins", "job", "migrate"]`), so the leaf's manifest
`group` (`"jenkins"`) is two levels up, not one. The lookup tries the
**more-specific** group first and only falls back to the shorter form if no
command matches, so a hypothetical project with both `docker.build` and
`docker.image.build` can never silently resolve the wrong one:

```rust
let candidates: &[String] = &[
    path[..path.len() - 1].join("."),
    if path.len() >= 2 {
        path[..path.len() - 2].join(".")
    } else {
        String::new()
    },
];
// Try each candidate group in order (longest/most-specific first). For
// each, look up by exact (group, name) match. First match wins.
let cmd = candidates
    .iter()
    .find_map(|group| {
        manifest
            .commands
            .iter()
            .find(|c| &c.group == group && c.name == leaf_name)
    })
    .ok_or_else(|| anyhow::anyhow!("unknown command: {}", path.join(" ")))?;
```

`parent_matches` (already tracked by the existing walk) ends up pointing at
the dispatcher's `ArgMatches`, which contains the dispatcher's own kwargs.
`build_dispatch_spec(dispatcher, parent_matches, packed, ...)` is unchanged
— it just receives a path-resolved-correctly dispatcher.

## Edge cases out of scope

1. **Bare `toolr <group> <dispatcher>`** — `subcommand_required(true)` rejects at parse time. Good UX. No widening of `invoke_dispatcher` semantics.
2. **Two dispatchers in one group** — covered by the `(module, function)` bucketing in 3c; each dispatcher hosts its own children. Unit-tested below.
3. **Dispatcher + sibling normal leaf in one group** — the dispatcher hosts grafted children; the normal leaf stays a sibling under the group. Unit-tested.
4. **Auto-rebuild race** between parallel `toolr <user-cmd>` invocations — both rebuild; the existing atomic-rename in `write_manifest` makes the last-writer-wins result correct.
5. **Auto-rebuild on a *corrupted* manifest** — `load_manifest` fails for non-`not-found` reasons. Current `load_or_empty` swallows the error to empty. This spec does not change that behaviour; only missing-manifest triggers the new bootstrap.

## Test surface

| Layer | Test | File | Coverage |
|---|---|---|---|
| Argv inspector | `should_skip_auto_rebuild` table | `crates/toolr/src/bootstrap.rs` | 10 cases (`--help`, `-h`, `--version`, bare, `__complete`, `project`, `self`, `init`, user-cmd, leading `--debug`) |
| Manifest field | `Command` serializes / omits `is_dispatcher` | `crates/toolr-core/src/manifest/tests.rs` | Mirror existing `dispatched_from` round-trip tests |
| Graft result | `run_for_project` returns `dispatchers` set | `crates/toolr-core/src/argparse/mod.rs` tests | Extension of Task-14 test |
| Manifest flag set | `build_static_manifest_grafts_argparse_children` extended | `crates/toolr-core/src/parser/build.rs` | `assert!(django.is_dispatcher)` |
| CLI tree — basic | Dispatcher + 2 children → dispatcher has both as subcommands | `crates/toolr/src/cli.rs` | Pure cli.rs unit test, no fixture-on-disk |
| CLI tree — two dispatchers in one group | Each dispatcher hosts its own children | `crates/toolr/src/cli.rs` | Unit |
| CLI tree — dispatcher + sibling normal leaf | Coexistence | `crates/toolr/src/cli.rs` | Unit |
| Dispatch path lookup | 3-segment path resolves correctly | `crates/toolr/src/dispatch.rs` | Likely covered by E2E |
| E2E auto-rebuild | Unskip `test_e2e_auto_rebuild_runs_argparse` | `tests/sources/test_e2e.py` | Already authored; just remove the `@pytest.mark.skip` |
| E2E outer flags | New: `toolr django job --cpu 5000m migrate --check`, sidecar asserts both | `tests/sources/test_e2e.py` | New test |

## Failure modes

| Failure | Behaviour |
|---|---|
| Auto-rebuild fails (venv broken, plugin error) | `rebuild_manifest_full` error propagates. `main.rs` prints `toolr: <error>` and exits 2. Same shape as `toolr project manifest rebuild` failing today. |
| Auto-rebuild succeeds but produces a manifest that still doesn't have the user's command | Clap rejects with "unrecognized subcommand" as today. Side-effect: the user now has a manifest. |
| Dispatcher built with `subcommand_required(true)` but the user typed only the dispatcher name | Clap prints "Usage: toolr jenkins job <COMMAND>" + the list of children. |
| User adds a new argparse-discoverable file but doesn't run rebuild | Same as today — tab completion stays stale until the next rebuild. Auto-rebuild does **not** fire because the manifest already exists. |

## Rollout sequence

Two stacks of commits inside one PR. They can land in either order; the plan
will sequence Stack A first because it's smaller.

**Stack A — auto-rebuild (gap 1):**

1. Add `should_skip_auto_rebuild` + 10-case table-driven tests.
2. Add `ensure_manifest_present_or_bootstrap` and wire into `main.rs::run` between `discover_project_root` and `load_or_empty`.
3. Unskip `test_e2e_auto_rebuild_runs_argparse`.

**Stack B — dispatcher hosts children (gap 2):**

1. Add `is_dispatcher: bool` to `Command` + round-trip tests + populate all literal sites.
2. Reshape `argparse::run_for_project` to return `GraftResult { children_by_parent, dispatchers }`; pass through to `build_static_manifest_inner` to flip the flag.
3. Reshape `cli.rs::build_group_subtree` + new `build_dispatcher_command` helper. Add the 3 cli.rs unit tests.
4. Fix `dispatch.rs` path lookup to handle 3-segment paths.
5. New E2E test exercising dispatcher outer flags.

## Out of scope (future work)

- Allowing a dispatcher to run *without* a child subcommand (would require widening `invoke_dispatcher` to construct a `DispatchCommand` with `command=""` or making the kwarg optional in the dispatcher's signature).
- Auto-rebuild on corrupted manifest (vs missing).
- Marking dispatchers from non-argparse sources (e.g. future Jenkins plugin). Adding `is_dispatcher = true` from a different graft path would Just Work — no design change needed.
