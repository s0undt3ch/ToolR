# Cache layout

Cached per-repo virtualenvs live at `$XDG_CACHE_HOME/toolr/` (or
`~/.cache/toolr/` on platforms without `XDG_CACHE_HOME`).

## Layout

```text
$XDG_CACHE_HOME/toolr/
├── <repo-key-1>/
│   ├── meta.json
│   └── venv/
├── <repo-key-2>/
│   ├── meta.json
│   └── venv/
└── <repo-key-3>-in-tree/
    └── meta.json
```

A repo with an in-tree venv (`venv-location = "in-tree"`) keeps its venv at
`tools/.venv`, so its entry holds only `meta.json`. The `-in-tree` suffix keeps
that entry apart from the one the repo's cache venv used: after a switch to
in-tree, the old `venv/` stops being touched and ages out like any stale entry.
`toolr self cache prune` on an in-tree entry removes the sidecar, not the venv.

toolr's managed copy of uv lives in the data directory, not here. See
[Finding uv](../installation/index.md#finding-uv).

`<repo-key>` is a stable, content-addressable identifier derived from
the repo's canonical path and Python version. **Each git worktree gets
its own cache entry** — the key is the worktree's own canonical path,
not anything shared with the repo it was branched from. This is
deliberate: worktrees can carry different `tools/` dependencies on
different branches, and sharing one venv across them would mean
constant resyncing (and, since nothing locks the venv directory, a
race between worktrees running toolr concurrently). Run `toolr self
cache prune` after removing a worktree to reclaim its venv.

## `meta.json`

Written when toolr creates a venv; updated on every invocation.

```json
{
  "repo_path": "/Users/you/code/my-repo",
  "toolr_version": "0.11.0",
  "python_version": "3.11.11",
  "created_at": "2026-04-02T14:23:11Z",
  "last_used_at": "2026-05-12T09:18:42Z"
}
```

- **`repo_path`** — the absolute path of the repo this venv was
  built for. Used to detect orphans.
- **`toolr_version` / `python_version`** — recorded for debugging.
- **`created_at`** — set once when the venv is first materialised.
- **`last_used_at`** — touched on every toolr invocation that hits
  this venv. Used to detect stale entries.

## Orphan detection

A cache entry is **orphan** when `meta.repo_path` no longer exists on
disk — typically because the repo was moved, renamed, or deleted.
`toolr self cache prune` lists orphans and (with confirmation)
removes them.

## Stale detection

A cache entry is **stale** when `last_used_at` is older than the
configured threshold (default 30 days). Override with
`--stale-after-days N` on `toolr self cache prune`.

## Pruning

```sh
toolr self cache list                       # show every entry
toolr self cache prune                      # remove orphans + stale
toolr self cache prune --dry-run            # show what would be removed
toolr self cache prune --all --yes          # nuke everything
toolr self cache prune --stale-after-days 7 # override threshold
```

Every prune also removes the manifest rebuild locks under `locks/` that no
remaining entry uses. A lock held by a running rebuild stays.

See [CLI reference → `self cache prune`](../cli.md#self-cache-prune)
for the full flag matrix.

## Passive orphan hint

When more than 10 cache entries belong to repositories that no longer exist,
toolr prints a one-line hint to stderr on the next invocation:

```text
toolr: cache has 12 orphan entries. Run `toolr self cache prune` to clean up.
```

The hint reads each entry's `meta.json` only. It never walks the virtualenvs,
so it adds no measurable time, and it does not mention sizes; `toolr self cache
list` shows those. A large cache with few orphans prints nothing.

The hint is skipped for:

- `--version`, `-V`, `--help` and `-h`
- `-q` / `--quiet`
- tab completion
- `toolr self cache ...` commands
- any run with `TOOLR_NO_CACHE_HINT` set, for example `TOOLR_NO_CACHE_HINT=1`

Arguments after a `--` separator are never read as toolr's own flags.
