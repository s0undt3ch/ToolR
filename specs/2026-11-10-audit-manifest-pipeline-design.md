# One manifest rebuild pipeline — design

Audit work package WP10, findings R-04 and R-13. Stacked on the quick-deletions branch.

## Why

`tools/.toolr-manifest.json` is built by five code paths, each with its own rules for the lock,
the snapshot clock, the `third_party_hash` stamp and what it carries forward from the cache:

1. `bootstrap::ensure_manifest_present_or_bootstrap`: missing manifest, trusted repo. It globs the
   venv only when `pyvenv.cfg` exists and never stamps `third_party_hash`, so it writes `""`.
2. `bootstrap::ensure_manifest_fresh` + `try_rebuild` + `carry_forward_cached_entries`: stale
   manifest, every non-built-in run.
3. `manifest_build::rebuild_manifest_full`: `project manifest rebuild`.
4. `dispatch::run_build_static_manifest`: the hidden `__build-static-manifest` command. It takes
   no venv, ignores trust and stamps no hash.
5. `complete::freshness::resolve_manifest_at_tab` + `preserve_non_static_entries`: TAB, in memory.

Both findings still hold on this branch. One more fact sharpens R-04: because (1) writes
`third_party_hash: ""`, step (2) in the same process always sees `ThirdPartyDrift` and builds
the tree a second time. That is why `main.rs` has to choose whose warnings to print. The result
is right, but every fresh clone pays for two builds.

## Key observation

`Origin` has two variants, and `freshness::compare` returns `StaticDrift` only when no venv is
passed. So the verdict only decides *whether* to rebuild. *How* to build depends only on whether
there is a venv:

- **venv given:** a static build plus the third-party glob, stamped with
  `compute_third_party_hash(venv)`. Nothing is carried forward.
- **no venv:** a static build that carries forward the cached `ThirdParty` entries (where no
  fresh entry has the same identity), the cached `plugin_warnings` and the cached
  `third_party_hash` (or `empty_third_party_hash()` when there is no cache).

The two carry-forward functions match this rule, so one function replaces both.

## Change

In `toolr_core::manifest_build::rebuild`:

- `fn build(tools, venv: Option<&Path>, cached: Option<&Manifest>) -> Result<BuiltManifest>`
  (private) applies the rule above. It owns hash stamping and carry-forward.
- `pub fn refresh(root, venv: Option<&Path>, persist: bool) -> Result<Refreshed>`, the automatic
  path. It loads the cache and runs `compare`. When the cache is stale, it builds. When
  persisting, the check goes through `rebuild_if_stale` (check, lock, re-check), the build through
  `build_to_persist` (clock first), and the result is written. When not persisting, there is no
  lock, no clock and no write. The result is one of:
    - `Fresh(Manifest)`
    - `Built(BuiltManifest)`
    - `Failed { error, cached }` for a build error, except symlinked modules, which return `Err`
    as they do on every path today

  `Refreshed` also carries the first non-`NotFound` load error, so the caller can warn about an
  unreadable manifest once.
- `pub fn rebuild_manifest_full(root, venv) -> Result<BuiltManifest>`, the explicit path. It
  always builds: lock, clock, `build(…, Some(venv), None)`, write. The `RebuildOutcome` struct
  goes away; the caller counts groups and commands itself.
- `BuiltManifest::warnings()`: the build warnings followed by the plugin warnings, with control
  characters stripped. `main.rs` and `project manifest rebuild` print from it.

Callers:

- **R-04, bootstrap.** `ensure_manifest_present_or_bootstrap` and `ensure_manifest_fresh` merge
  into one `ensure_manifest(cwd, argv, persist)` over `refresh`. Its only policy:
    - argv skip-list
    - unreadable-manifest warning
    - `Failed` with no manifest file in a trusted repo is fatal (the bootstrap's behaviour today)
    - any other `Failed` warns and falls back to the cache

  `main.rs` makes one call, with no bootstrap-versus-freshness warning choice. A fresh clone
  builds once.
- **R-04, TAB.** `resolve_manifest_at_tab` calls `refresh(root, None, false)` and ignores the
  load error, as it does today. `Failed` serves the cache, or errors when there is none.
  `preserve_non_static_entries` goes.
- **R-04, explicit.** `project manifest rebuild` keeps its venv and its error context
  (`building static manifest (incl. third-party fragments)`).
- **R-13.** Delete `__build-static-manifest`: the command, its dispatch arm, its hidden-command
  entries and its `docs/cli.md` section. The tests that built a manifest with it now run
  `project manifest rebuild` through a `common::build_manifest(root)` helper. Some test rows
  existed only to cover the command itself: the symlink and warnings rows in `cli_smoke.rs`, the
  `-q` row in `root_quiet_builtins.rs` and the hidden-command lists. Those rows are dropped.
- **Error text.** `BuildError::into_anyhow` now unwraps `BuildError::Build` to its inner error,
  the way `build_static_manifest` already does, and `build_static_manifest` calls it. Today a
  parse error reads differently depending on the path. It is
  `static build error: parsing X: parsing X: …` with a venv and `parsing X: …` without one, and
  the fallback warning's cause line is `static build error: parsing X`. After this change every
  path prints `parsing X: …` (cause line `parsing X`). This is user-visible, so it gets a release
  note. The characterisation tests pin today's text first, so the diff shows the change.

## Behaviour kept (pinned by tests)

- Trusted fresh clone with a broken module: exit 2 with the error, nothing written.
- Untrusted repo: built in memory, no lock, no clock, no write. A broken build warns and falls
  back.
- A stale manifest whose rebuild fails warns, points at `project manifest rebuild` and serves the
  cache. Symlinked modules are fatal everywhere.
- The manifest written on the first run stays fresh on the next run and is not rewritten.
- `project manifest rebuild` always builds and waits on the repo's rebuild lock.
- Concurrent auto-rebuilds rebuild once under the lock (existing tests).
- TAB never writes, never globs the venv, and serves the cache on a failed build.
- `self build-manifest` and `--check` build plugin fragments, not this manifest. They are out of
  scope and unchanged.

Edge cases that change:

- `resolve_venv_path` fails (broken pyproject) on a fresh clone. The written hash becomes
  `empty_third_party_hash()` instead of `""`, which stops a pointless second build.
- A third-party hash I/O error during `project manifest rebuild` gains a
  `hashing third-party manifests` context line.

## Non-goals

- `hash.rs` and the freshness comparison itself.
- Argv pre-parsing (R-05) and `ProjectCtx` (R-03).
- The build-fragment pipeline behind `self build-manifest`.

## Line counts

Filled in from `git diff --stat` against the base branch once the implementation lands.
