# Key the action's tools-venv cache on `tools/` only

- **Status:** implemented
- **Origin:** whole-project review finding F-39 (low).
- **Plan:** [`2026-10-31-action-cache-key-plan.md`](./2026-10-31-action-cache-key-plan.md)
- **Stacks on:** `fix/action-latest-repo` (F-22, [`2026-10-31-action-latest-repo-design.md`](./2026-10-31-action-latest-repo-design.md)); merged in #624.

## Problem

The Setup ToolR action's "Cache tools venv" step (`action.yml`, step id `venv-cache`) keys the in-tree
`tools/.venv` cache on:

```yaml
key: ${{ inputs.cache-prefix }}-tools-venv|${{ steps.archive.outputs.triple }}|${{ hashFiles('tools/pyproject.toml', 'tools/uv.lock', 'uv.lock') }}
```

Its comment justifies the root `uv.lock` with "this is what the toolr repo itself uses — `tools/` is a uv workspace
member of the root". That is false. `tools/pyproject.toml` says `tools/` is intentionally not part of the root
workspace, and the repo commits a standalone `tools/uv.lock`. toolr itself only ever looks at `tools/uv.lock`:
`check_freshness` in `crates/toolr-core/src/venv/sync.rs` stats `tools_dir.join("uv.lock")` and returns `Missing`
without it, so a layout that relies on a root lock is not one toolr supports.

The effect: every change to a repo's root `uv.lock` (in toolr's own repo, every Renovate bump of a dev dependency)
changes the key, so every CI job that uses the action misses the tools-venv cache and rebuilds a venv whose contents
didn't change. The `cache-tools-venv` input description already says "keyed on `tools/pyproject.toml` and
`tools/uv.lock`", so the key, not the documentation, is what's out of line. The skill repeats the wrong key:
`skills/toolr-ci-setup/SKILL.md`, "Common failure modes", says the key hashes "`tools/pyproject.toml`,
`tools/uv.lock`, and `uv.lock`".

Repro: two commits that differ only in the root `uv.lock` produce different `hashFiles(...)` values, so the second
run's "Cache tools venv" step logs a cache miss for an identical tools venv.

## Decision

Key on the same `tools/` inputs toolr's own freshness check tracks (`uv.lock` plus `SYNC_INPUTS` in
`crates/toolr-core/src/venv/sync.rs`):

```yaml
key: ${{ inputs.cache-prefix }}-tools-venv|${{ steps.archive.outputs.triple }}|${{ hashFiles('tools/pyproject.toml', 'tools/uv.lock', 'tools/uv.toml', 'tools/.python-version') }}
```

- Rewrite the step comment: the key hashes the `tools/` inputs toolr's freshness check tracks, and the root lock is
  deliberately left out.
- Fix the `SKILL.md` failure-mode bullet to name those files, and correct its diagnosis: `hashFiles` skips absent
  files, so a missing `tools/uv.lock` fails the `--frozen` sync rather than destabilising the key.
- Update the `cache-tools-venv` description to list the same files and regenerate `references/action.md`.

(The first cut hashed only `tools/pyproject.toml` and `tools/uv.lock`; review widened it to match the freshness
check.)

### Rejected alternatives

- **Hash `tools/uv.lock` when it exists, else the root `uv.lock`.** It keeps a "workspace member" layout cacheable,
  but toolr's freshness check already treats that layout as `Missing` on every run. A cache branch for a layout the
  binary doesn't support adds a code path nobody exercises.
- **Keep the key, fix only the comment.** Leaves the cache churn, which is the actual cost.
- **Ask `uv` which lock it uses** (`uv sync --project tools --dry-run` or similar) and hash that. More moving parts in
  a composite step for the same answer as the hardcoded path.

## Scope and compatibility

- No manifest or runner schema change.
- The key's hash input changes, so every caller misses the tools-venv cache once after upgrading the action, then hits
  again. No `cache-prefix` bump needed.
- Assumes `fix/action-latest-repo` changed only the `resolve-version` step and the `version` input description. This
  change touches only the `venv-cache` step and `SKILL.md`; the two don't overlap.
- Docs: `skills/toolr-ci-setup/SKILL.md` ("Common failure modes", the `tools/.venv` cache bullet) and the
  regenerated `skills/toolr-ci-setup/references/action.md`.
- `UNRELEASED.md`:

  > The Setup ToolR action no longer throws away its `tools/.venv` cache when the repository's root `uv.lock`
  > changes. The cache key now follows the same `tools/` inputs toolr's own freshness check tracks:
  > `tools/pyproject.toml`, `tools/uv.lock`, `tools/uv.toml` and `tools/.python-version`. Expect one cache miss after
  > upgrading the action.
