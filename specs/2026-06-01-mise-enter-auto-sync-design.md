# Auto-sync the tools venv from a mise enter-hook

**Status:** design — pending implementation plan
**Target release:** 0.22 (breaking)
**Related ticket:** TBD — broader `toolr project venv` uv-wrapper expansion (filed separately; see "Follow-ups")

## Problem

A toolr user pulls a branch where `tools/pyproject.toml` or `tools/uv.lock` has
changed. Today their next `toolr <anything>` invocation has to detect the drift
and either run a sync itself or hand back an error pointing at
`toolr project deps sync`. The user pays the latency on a command they ran for
a different reason, and may hit a confusing failure mid-flow if the drift
manifests as a missing import rather than a freshness verdict.

What the user actually wants is **the venv is fresh by the time my prompt
returns after `cd` into the project**. That's what mise's `[hooks].enter`
gives them — a hook that fires on shell-enter, before they run anything.
Plugging toolr into that hook is straightforward in principle, but two
shapes of it (raw `uv sync`, mise sources/outputs gating) have subtle
correctness traps. This design picks the shape that's correct *and* cheap.

## Goals

- A documented one-line recipe a user can paste into `mise.toml` so their
  tools venv stays in sync on shell-enter, with imperceptible cost when
  nothing changed.
- The enter-hook respects toolr's `[tool.toolr] venv-location` setting (cache
  vs in-tree) and never creates a divergent `tools/.venv/` behind toolr's
  back.
- The enter-hook never blocks the shell — no prompts, no errors that surface
  as scary stderr on every `cd`, no multi-second first-run sync without the
  user noticing.
- The user-visible command surface for tools-venv operations is reshaped
  under `toolr project venv` so future uv-wrapper subcommands (`add`,
  `remove`, `lock`, …) land in the obvious place.

## Non-goals

- Wrapping additional uv operations (`add`, `remove`, `lock`, etc.). The
  command-tree reshape this design ships makes that future work cheap, but
  it is **out of scope here** and lives in a separate ticket (see "Follow-ups").
- Linking against uv as a library. The CLI is uv's only semver-promised
  interface; subprocess overhead (~30–50 ms per call) is well below human
  perception. Embedding uv crates would inflate the toolr binary, couple
  toolr to uv-internal API churn, and still not avoid spawning a Python
  interpreter for many ops. Not worth it.
- Auto-injecting the enter-hook into a user's `mise.toml`. toolr distributes
  via the aqua-registry, so there is no plugin install hook from which to
  edit the user's project config. The user pastes the line themselves (or a
  future `toolr project init --mise` scaffold can append it; not in this
  spec).
- Changing where the freshness stamp lives. The existing
  `<venv_dir>/.toolr-sync-stamp` design stays; see "Rejected
  alternatives" for the "move to `tools/.toolr-sync-stamp`" option that
  was discarded.

## Background

### How toolr knows the venv is stale today

`crates/toolr-core/src/venv/sync.rs` writes `.toolr-sync-stamp` (empty file)
into the resolved venv directory after each successful `uv sync`. The mtime
of that marker is compared against the mtime of `tools/uv.lock`:

| Stamp / Lock state | `Freshness` |
| --- | --- |
| Stamp absent **or** venv absent | `Missing` |
| Stamp older than lock | `Stale` |
| Stamp newer-or-equal to lock | `Fresh` |

`sync_if_needed(force=false)` short-circuits on `Fresh`; with `force=true`
it always runs uv. The only reader of the stamp is `check_freshness()`.
Callers of `sync_if_needed()`:

- `ensure_venv_ready()` — the gate any path that needs the venv goes through
- `toolr project deps sync` (today: passes `force=true`)
- `toolr project venv shell` (today: passes `force=false`)
- the dispatcher before running a Python command

Nothing else in the codebase consults the stamp. Cache inspection
(`crates/toolr-core/src/cache/`), `toolr project venv path`, and the
manifest-freshness path (`bootstrap.rs`, `complete/freshness.rs`) all use
unrelated machinery.

### What mise's enter-hook does

mise reads `[hooks].enter` from the user's `mise.toml` and runs the string
as a shell command each time mise activates in that directory. It's the
right injection point for "run on `cd` into a repo". mise also offers
`[tasks]` with `sources`/`outputs` for Make-style up-to-date skipping, but
this design does not use that pattern (see "Rejected alternatives") because
the resulting cost difference (~35 ms) is imperceptible and the extra
config drift isn't worth the savings.

### Why the hook can't just run `uv sync`

`uv sync --project tools/` with no environment override creates and syncs
**`tools/.venv/`** — uv's standard project layout. toolr's
`run_uv_sync()` sets `UV_PROJECT_ENVIRONMENT` to the resolved venv path
(cache or in-tree per `[tool.toolr] venv-location`). A naive recipe that
ran raw `uv sync` would silently materialise a sibling venv that toolr
neither knows about nor uses, leaving the toolr-managed venv to drift
forever. The hook **must** go through a toolr entry point so the same
`venv-location` resolution applies.

## Solution

Three threads that ship together at 0.22:

1. **Command-tree reshape.** Collapse `toolr project deps` into
   `toolr project venv`.
2. **Behavior flip on `sync`.** Default becomes idempotent
   (no-op when fresh); `--force` is the explicit always-run.
3. **Quiet, unattended-safe mode.** `--quiet` adds the guards needed to
   make `sync` safe to run from a shell-enter hook on every `cd`.

### Command-tree reshape

Before:

```text
toolr project
  init
  deps
    sync
    upgrade
  venv
    path
    shell
  manifest
    …
```

After (0.22):

```text
toolr project
  init
  venv
    path                       (unchanged)
    shell                      (unchanged)
    sync                       (moved from `project deps sync`; behavior flipped)
    sync --force               (today's `deps sync` semantics, behind a flag)
    sync --quiet               (new — silent on no-op; implies unattended)
    upgrade <package>          (moved from `project deps upgrade`; unchanged)
  manifest
    …
```

The `project deps` group is removed wholesale. Invoking
`toolr project deps <anything>` at 0.22 returns a clap-level
"unrecognized subcommand" error with a tailored hint that points at the new
path:

```text
$ toolr project deps sync
error: `project deps` was removed in 0.22
hint: use `toolr project venv sync` instead
see CHANGELOG.md (0.22 BREAKING) for the rename
```

No deprecation shim. Pre-1.0 minor bumps are exactly the version slot for
this kind of break. The CHANGELOG entry under `BREAKING` calls it out, and
the `UNRELEASED.md` working draft references the design doc.

### Behavior flip on `sync`

Today `toolr project deps sync` passes `force_sync=true` straight through
to `sync_if_needed`. After this change:

| Invocation | `force_sync` | Output when fresh |
| --- | --- | --- |
| `toolr project venv sync` | `false` | uv lines + "toolr: synced venv at …" |
| `toolr project venv sync --force` | `true` | uv lines + "toolr: synced venv at …" |
| `toolr project venv sync --quiet` | `false` | silent on no-op; errors still print |
| `toolr project venv sync --force --quiet` | `true` | uv inherits `--quiet`; toolr line suppressed |

When `force_sync=false` and `check_freshness` returns `Fresh`, `sync_if_needed`
exits without spawning uv at all. Cost on a fresh venv: one toolr process,
~30–35 ms wall-clock, no subprocesses.

### `--quiet` semantics (unattended mode)

`--quiet` is more than "shut up." It is the contract that the caller is
running in an unattended context (a mise enter-hook, a CI smoke check)
and that toolr must never:

- Block waiting for a TTY prompt (e.g. uv-install consent).
- Emit non-error output. Errors and warnings still go to stderr; success
  paths produce no stdout.
- Surface a hard error for situations that are "not toolr's problem to
  fix right now" — see the guard table below.

Concretely, when `--quiet` is set, `sync` exits **0 silently** in these
cases:

| Situation | Without `--quiet` | With `--quiet` |
| --- | --- | --- |
| Not inside a toolr-using repo (no `tools/pyproject.toml`) | normal error | exit 0 silent |
| `tools/uv.lock` missing | normal error | exit 0 silent |
| uv not on PATH **and** auto-install consent absent | TTY prompt | exit 0 silent (no prompt) |
| Lock file unparsable | normal error to stderr | error to stderr, exit non-zero |
| `uv sync` returns non-zero | normal error to stderr | error to stderr, exit non-zero |
| `Freshness::Fresh` | "toolr: synced venv at …" line | exit 0 silent |
| `Freshness::Stale`, uv sync succeeds | uv output + toolr line | suppress toolr line; uv inherits `--quiet` |

The principle: `--quiet` only swallows situations the **user can fix by
running `toolr project venv sync` manually**. Genuine errors (lock
unparsable, uv sync failed) still escape so they're not silently masked.

### mise integration recipe (docs only)

Documentation addition to `docs/installation/mise.md`. The recipe is one
block:

```toml
# In your project's mise.toml
[hooks]
enter = "toolr project venv sync --quiet"
```

That's it. No `[tasks]` block, no `sources`/`outputs` configuration. The
recipe works identically for `venv-location = "cache"` (default) and
`venv-location = "in-tree"` users — the stamp's location follows the venv.

Docs cover:

- What the hook does (one paragraph).
- The unattended-mode guards (so users understand "why nothing happened
  when uv wasn't installed").
- A one-time bootstrap step: run `toolr project venv sync` manually once
  per project to install uv (with prompt) and materialise the venv. From
  then on, the enter-hook keeps it fresh.
- A note that the hook is project-scoped — it lives in the project's
  `mise.toml`, not in a global mise config.

## Implementation outline

In rough dependency order, owned by the implementation plan:

1. **Rename clap subcommands.** Move `project deps sync` and
   `project deps upgrade` under `project venv`. Drop the `project deps`
   group from the clap tree. Add the tailored error path that hints at the
   new location for the removed subcommand string.
2. **Add `--force` and `--quiet` to `project venv sync`.** Wire `--force`
   to the existing `force_sync` parameter (flip the default to `false`).
   `--quiet` adds an `Unattended` config struct passed through
   `ensure_venv_ready` / `sync_if_needed`.
3. **Implement the unattended guards.** Each row in the guard table
   becomes a guard check inside the new `sync` entry-point or in
   `ensure_venv_ready`. Most are simple early-return-Ok paths; the
   uv-consent guard threads through `ConsentMode`.
4. **Update tests.**
   - `crates/toolr/tests/project_deps_upgrade.rs` → rename to
     `project_venv_upgrade.rs`; update paths.
   - Add `crates/toolr/tests/project_venv_sync.rs` covering: fresh
     no-op (no uv spawn), stale → uv spawn, `--force` always spawns,
     `--quiet` suppresses output on fresh, `--quiet` silences
     unattended-guard exits.
   - Add a test for the `deps` → `venv` migration error.
5. **Docs.** Update `docs/installation/mise.md` with the enter-hook
   recipe and unattended-mode note. Update `docs/reference/` listings
   to reflect the new command paths. Update `UNRELEASED.md` /
   `CHANGELOG.md` with the breaking entry.
6. **Cleanup.** Search for any internal call sites still using
   `project deps` strings (completion fixtures, smoke tests, doc
   fixtures); update them.

## Test plan

Unit-level (Rust):

- `sync_if_needed_skips_run_when_fresh_and_force_off` (already exists; keep).
- New: `sync_quiet_exits_silently_when_missing_tools_dir`.
- New: `sync_quiet_exits_silently_when_uv_install_consent_absent`.
- New: `sync_quiet_propagates_uv_sync_failures` (errors still surface).
- New: `removed_deps_subcommand_emits_migration_hint`.

Integration-level (`crates/toolr/tests/`):

- `project_venv_sync.rs` end-to-end: stale lock → uv runs; second
  invocation → no uv subprocess (assert via stub-uv invocation count).
- Recipe smoke: spawn a shell with the documented `[hooks].enter` line and
  assert that two consecutive entries result in exactly one uv spawn (the
  first one). This may live as a `#[ignore]`-by-default test gated on a
  local mise install, or as a documentation-only assertion.

Doc-level:

- mkdocs strict build passes with the rewritten `installation/mise.md`.
- skill reference regeneration (`cargo xtask build-skill-refs --check`)
  is clean.

## Risk and rollback

- **Risk: users with `toolr project deps …` muscle memory or scripts.**
  Mitigation: the migration-hint error message names the new path
  explicitly. CHANGELOG calls it out under BREAKING. Pre-1.0 contract
  means scripts pinning a specific minor were already accepting churn.
- **Risk: a poorly-written `--quiet` guard masks a genuine failure.**
  Mitigation: the guard table is restrictive — only `tools/`-missing,
  `uv.lock`-missing, and uv-consent-absent get the silent-exit treatment.
  Lock-unparsable and uv-sync-failed still error normally.
- **Risk: the enter-hook recipe surprises users who rely on
  `tools/.venv/` materialising as a side-effect of some other workflow.**
  Mitigation: the recipe is opt-in (paste, don't auto-inject). Docs
  spell out that the hook respects `venv-location` and won't create a
  sibling `.venv`.

Rollback path: the change is contained in clap definitions and the
`sync_if_needed` call site. Reverting the PR restores the prior surface
in one commit.

## Rejected alternatives

### Move the stamp to `tools/.toolr-sync-stamp` and use mise's sources/outputs

Initial sketch had the user paste a fuller recipe:

```toml
[hooks]
enter = "mise run --silent toolr-sync"

[tasks.toolr-sync]
hide = true
sources = ["tools/pyproject.toml", "tools/uv.lock"]
outputs = ["tools/.toolr-sync-stamp"]
run = "toolr project venv sync --quiet"
```

This would skip the toolr spawn entirely when fresh (mise compares the
mtimes itself and never invokes the task). The cost saved: ~35 ms per
shell-enter. The cost paid: the freshness stamp has to move from
`<venv_dir>/.toolr-sync-stamp` to `tools/.toolr-sync-stamp` so the mise
`outputs` glob can reference it independently of the user's
`venv-location`, **and** `check_freshness` needs an additional
`venv_dir/pyvenv.cfg` exists-check so that deleting the cache venv
doesn't leave a stale-stamp-with-missing-venv false-fresh.

Rejected because 35 ms is imperceptible, the extra recipe lines are real
config drift in users' mise.toml files, and the schema change to the
stamp location (plus its new venv-existence guard) is meaningful surface
to maintain for no user-visible benefit.

### Add a separate `toolr project venv ensure` subcommand

Considered keeping `venv sync` as a force-sync alias (matching today's
`deps sync` semantics) and adding `venv ensure` for the
sync-if-stale path the enter-hook wants. Two subcommands with very
similar names invite confusion ("why ensure not sync?") and double the
help surface. Folding both behaviors under one `sync` command — with
`--force` as the explicit opt-out from the new default — is one fewer
concept to learn.

### Link against uv as a library

Discussed and discarded. Reasons:

- uv has no stable embedding API; the `uv-*` crates on crates.io are
  internal and change shape between releases. The CLI is the only
  semver-promised surface.
- Embedding would meaningfully grow toolr's binary and compile time
  (pubgrub, PEP 517/660 builder, PyPI client, etc.).
- Many uv operations spawn a Python interpreter anyway; the subprocess
  boundary cannot be removed for them.
- Subprocess overhead (~30–50 ms) is well below human perception.

### Auto-inject the enter-hook from `toolr project init`

Plausible, but not for this spec. With toolr shipping via aqua-registry,
the only place we could write this line is from the `project init`
scaffold (mise plugin install-time hooks aren't a thing for aqua-backed
tools). Adding `--mise` to `project init` is a worthwhile follow-up but
orthogonal to the design goal here, which is "make the recipe exist and
work correctly." Decoupling lets us ship the recipe + behavior in 0.22
and revisit auto-scaffolding once the recipe has miles on it.

## Follow-ups

- File a separate GitHub issue capturing the broader vision the rename
  enables: wrap remaining uv operations under `toolr project venv` —
  `add`, `remove`, `lock`, and others as we discover them. Skip
  operations already implemented (`sync`, `upgrade`). The ticket is a
  placeholder; this design does not commit to that scope.
- Reconsider auto-scaffolding once the manual recipe has shipped and we
  have real usage data on what users get wrong about it.
- Once `venv add` / `venv remove` exist, revisit whether the enter-hook
  recipe should ever auto-respond to `pyproject.toml` edits made
  outside toolr (today the hook handles this via the existing stamp
  comparison; future uv-wrapper commands should keep the stamp
  authoritative).
