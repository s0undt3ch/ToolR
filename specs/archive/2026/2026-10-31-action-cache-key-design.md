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

Key on the two files that define the tools venv:

```yaml
key: ${{ inputs.cache-prefix }}-tools-venv|${{ steps.archive.outputs.triple }}|${{ hashFiles('tools/pyproject.toml', 'tools/uv.lock') }}
```

- Rewrite the step comment: the venv is invalidated by `tools/pyproject.toml` or `tools/uv.lock`, matching what
  toolr's own freshness check reads, and the root lock is deliberately left out.
- Fix the `SKILL.md` failure-mode bullet to name the two files.
- The `cache-tools-venv` description is already right; leave it, so the generated `references/action.md` doesn't
  change.

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
- Docs: `skills/toolr-ci-setup/SKILL.md` ("Common failure modes", the `tools/.venv` cache bullet). No generated
  reference change.
- `UNRELEASED.md`:

  > The Setup ToolR action no longer throws away its `tools/.venv` cache when the repository's root `uv.lock`
  > changes. The cache key now hashes only `tools/pyproject.toml` and `tools/uv.lock`, the files the tools venv is
  > built from. Expect one cache miss after upgrading the action.
