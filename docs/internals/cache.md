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
└── bin/
    └── uv          # toolr's managed copy of uv (optional)
```

`<repo-key>` is a stable, content-addressable identifier derived from
the repo's canonical path and Python version. Multiple worktrees of
the same repo share a single cache entry.

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

See [CLI reference → `self cache prune`](../cli.md#self-cache-prune)
for the full flag matrix.

## Passive size hint

When the cache exceeds a soft threshold (or accumulates orphans),
toolr prints a one-line hint to stderr on the next invocation:

```text
toolr: cache is 1.4 GB across 18 entries (6 orphan); run `toolr self cache prune` to clean up.
```

Suppress with `TOOLR_NO_CACHE_HINT=1` in your environment.

The hint logic is conservative — it only prints once per (cache size,
orphan count) bucket, so you won't get nagged on every invocation.
