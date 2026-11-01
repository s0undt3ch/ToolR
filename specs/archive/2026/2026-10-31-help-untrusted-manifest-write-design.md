# Don't write the manifest into an untrusted repository

- **Status:** implemented
- **Origin:** whole-project review finding F-27 (low-medium).
- **Plan:** [`2026-10-31-help-untrusted-manifest-write-plan.md`](./2026-10-31-help-untrusted-manifest-write-plan.md)

## Problem

Before clap parses anything, `main.rs::run` calls two bootstrap steps in `crates/toolr/src/bootstrap.rs`:

- `ensure_manifest_present_or_bootstrap` builds a missing manifest.
- `ensure_manifest_fresh` rebuilds a stale one.

Both write `tools/.toolr-manifest.json` with `write_manifest` (`crates/toolr-core/src/manifest/io.rs`). By design they
run for `--help` and bare `toolr` too (see the `should_skip_auto_rebuild` doc comment), so the help output shows the
repo's commands.

The repository-trust gate added in #612 (`specs/archive/2026/2026-10-30-repo-trust-design.md`) deliberately exempts
`--help`, bare `toolr`, manifest builds and TAB, because they only parse statically. So nothing stops these writes. A
read-looking command writes a file into a checkout the user has never trusted: a colleague's worktree, a shared or NFS
checkout, a freshly cloned repo being inspected. A dispatch that trust then refuses (`toolr demo hello`) has already
written the manifest in bootstrap, before `dispatch.rs` calls `require_trust`.

Repro, at `25e7ffdb`, with an empty trust store: in a repo with `tools/pyproject.toml` and `tools/demo.py`, run
`toolr --help`. The output lists `demo`, and `tools/.toolr-manifest.json` (mode 0644) now exists.

The write is atomic (a tempfile renamed over the target), so a planted symlink is replaced, not followed. This is about
side effects, not code execution.

## Decision

The bootstrap steps persist the manifest only when the repository is trusted. Otherwise they build it in memory, as TAB
does.

- New in `crates/toolr-core/src/trust.rs`: `pub fn is_trusted_now(root: &Path) -> bool`. It returns `trust_all_env() ||
  TrustStore::open_default()?.is_trusted(root)?`, and `false` on any error (no state dir, unreadable store,
  `StoreInsideRepo`). It never prompts, writes or prints. `open_default` only builds a path and `is_trusted` only reads
  one file, so no directory is created. If the store is unreadable, dispatch's `require_trust` still warns later.
- `ensure_manifest_present_or_bootstrap` computes `persist = is_trusted_now(&root)` after discovery and returns
  `Ok(())` when it is `false`. `ensure_manifest_fresh` then sees no manifest and builds one.
- `ensure_manifest_fresh` computes the same flag. When `persist` is `false`, it runs `check` once and, if stale,
  `try_rebuild`, without `rebuild_if_stale` and its lock. It returns `Some(fresh)`, so `main.rs` renders help or
  dispatches against the in-memory tree, and nothing is written.
- Explicit builds stay unconditional, because the user asked for them: `toolr project manifest rebuild`,
  `toolr self build-manifest` and the hidden static-build command (`dispatch.rs::run_build_static_manifest`).
- `TOOLR_TRUST_ALL=1` persists, so CI and the GitHub Action behave as before.

Consequences:

- The first run in an untrusted repo where the user answers `y` at dispatch's trust prompt runs the command without
  persisting the manifest. The next run persists it.
- A stale or corrupt manifest in an untrusted repo is left exactly as found. The existing "ignoring unreadable …;
  rebuilding" warning still fires, and the rebuild happens in memory.

### Rejected alternatives

- **Never persist on `--help` or bare `toolr`, trusted or not.** Persisting on root help is designed behaviour. About
  15 integration tests (`plugin_warnings.rs`, `freshness_dispatch.rs`, `untrusted_repo.rs`) rely on it, and trusted
  users get a cached manifest from it. It also leaves the refused-dispatch write in place.
- **Classify argv (`--help` anywhere, a group without a leaf).** Before the manifest exists, pre-clap code can't tell a
  user group from a typo, so a rule like that has edge cases that trust doesn't.
- **Write the manifest to the cache directory for untrusted repos.** That gives two sources of truth for one manifest,
  and the cache key is the same root.
- **Require trust for `--help`.** The trust design rejected this. Help is static and must work in a fresh clone.

## Scope and compatibility

- No manifest or runner schema change.
- Trusted repos and `TOOLR_TRUST_ALL=1` behave exactly as before.
- Untrusted repos: no manifest file is created or rewritten by `--help`, bare `toolr`, or a dispatch that trust
  refuses. Each such run reparses `tools/` statically, which takes milliseconds.
- Tests: `common::toolr()` sets `TOOLR_TRUST_ALL=1`, so existing integration tests pass unchanged. The one fixture
  change is `common::hostile_repo`. Its seeding `--help` call uses a raw `cargo_bin` command and relies on the write,
  so it needs `.env("TOOLR_TRUST_ALL", "1")`. That is a fixture fix, not a behaviour change.
- `fix/tab-manifest-freshness` (F-30) also edits `ensure_manifest_fresh`. Whichever branch merges second rebases.
- Docs: in `docs/concepts.md`, section "Repository trust", add that toolr never writes `tools/.toolr-manifest.json` into
  an untrusted repository; `--help` and tab completion build it in memory. In `docs/internals/manifest.md`, under
  "Toolr regenerates the manifest when", note that automatic rebuilds are written to disk only in trusted repos.
- `UNRELEASED.md`:

  > `toolr --help`, bare `toolr` and a refused command no longer write `tools/.toolr-manifest.json` into a repository
  > you haven't trusted. toolr builds the command list in memory there instead, so inspecting an unfamiliar checkout
  > leaves it untouched. Trusted repositories, and runs with `TOOLR_TRUST_ALL=1`, still cache the manifest as before.
