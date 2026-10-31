# Keep the in-tree venv's meta.json out of the repo

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-18 (medium).
- **Plan:** [`2026-10-31-in-tree-venv-cache-dir-plan.md`](./2026-10-31-in-tree-venv-cache-dir-plan.md)
- **Stacks on:** `fix/venv-sync-pyproject` (F-04,
  [`2026-10-30-venv-freshness-inputs-design.md`](./2026-10-30-venv-freshness-inputs-design.md));
  merge that first.

## Problem

Every command dispatch updates the venv's `meta.json` sidecar, which `toolr self cache list` and `prune` read. In
`run_command` (`crates/toolr/src/dispatch.rs`), the block after the venv is resolved derives the sidecar directory as
`venv.parent()` and passes it to `toolr_core::cache::touch_or_backfill`.

That is right for a cache-located venv: `<cache>/toolr/<repo-key>/venv` has the parent `<cache>/toolr/<repo-key>/`,
where the sidecar lives. For an in-tree venv, `tools/.venv` has the parent `tools/`. `touch_or_backfill` finds no
sidecar there, so it writes a new one:

```sh
toolr project init --venv-location in-tree
toolr example hello
git status --short    # ?? tools/meta.json
```

The stray `tools/meta.json` (mode 0600) is untracked. The scaffolded `tools/.gitignore`
(`init_templates/gitignore.tmpl`) ignores `.venv/` and `.toolr-manifest.json`, not this file. `toolr self cache list`
and `prune` never see it, because they only look under the cache root.

Sync already does this correctly. `toolr_core::project::meta_cache_dir` (the old `provenance_cache_dir`, renamed when
repo trust replaced interpreter provenance in #612) returns `venv_dir.parent()` for a cache venv and
`<cache>/toolr/<repo-key>/` for an in-tree one. `finalize_sync` writes the sidecar there. Dispatch is the only caller
that bypasses it, so the two disagree about where an in-tree repo's sidecar lives.

## Decision

Dispatch gets the sidecar directory from `toolr_core::project::meta_cache_dir(&repo_root, resolved)` and passes that to
`touch_or_backfill`. If `meta_cache_dir` fails (no cache directory can be resolved), dispatch prints the existing
`toolr: warning: failed to touch cache meta.json: …` line and carries on, as it does for a failed touch now.

`Meta::write` creates the directory when it is missing, so a repo dispatched before its first sync still gets the
sidecar in the right place.

F-04 changed `resync_if_needed` and `check_freshness`'s list of sync inputs in the same file. This change touches only
the sidecar block after them, so nothing overlaps. It needs the `ResolvedVenv` (`resolved_venv`), not just its
`venv_dir`, because `meta_cache_dir` reads `repo_key` and `python_version`.

### Rejected alternatives

- **Skip the touch for in-tree venvs.** Sync would still write an out-of-repo sidecar that dispatch never refreshes, so
  `self cache list` would show a stale "last used" and `prune --older-than` could delete a slot in active use.
- **Add `meta.json` to `gitignore.tmpl`.** It hides the file in new projects only, and the file still lands in the
  repo, out of `self cache`'s reach.
- **Delete a stray `tools/meta.json` on dispatch.** toolr can't tell its own leftover from a user file with that name,
  and deleting user files to tidy up isn't worth the risk. The release note tells users to remove it.

## Scope and compatibility

- No schema change. The runner spec is untouched.
- Behaviour change: in-tree repos stop getting `tools/meta.json`; their sidecar goes to the same out-of-repo slot sync
  already writes. That slot holds only `meta.json` (the venv stays in `tools/.venv`), so `toolr self cache list` lists
  in-tree repos, as it already does after a `venv sync`. `prune` on such a slot removes the sidecar, not the venv.
- Docs: none. No page describes where dispatch writes the sidecar.
- `UNRELEASED.md`:

  > Running a command in a repo with an in-tree venv (`venv-location = "in-tree"`) no longer leaves an untracked
  > `tools/meta.json`. toolr now keeps that file in its cache directory, where `toolr project venv sync` already put it.
  > You can delete any `tools/meta.json` an earlier toolr left behind.
