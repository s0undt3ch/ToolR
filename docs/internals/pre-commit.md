# Pre-commit integration

The toolr repo ships a `.pre-commit-hooks.yaml` so downstream
consumers can wire toolr-managed hooks into their own
`.pre-commit-config.yaml` without copy-paste.

## Available hooks

### `toolr-manifest`

Runs `toolr project manifest rebuild` whenever a file under `tools/`
changes. Ensures the committed manifest stays in sync with the
source code; without it, you'd ship stale tab-completion data and
risk `--help` output that no longer matches the implementation.

## Consumer configuration

Add the toolr repo to your `.pre-commit-config.yaml`:

```yaml
- repo: https://github.com/s0undt3ch/ToolR
  rev: v0.11.0  # use the latest released tag
  hooks:
    - id: toolr-manifest
```

The hook only fires on commits that touch `tools/*.py` files. Other
commits are unaffected — the cost is zero when you're not editing
tools.

## When the hook fails

`toolr-manifest` exits non-zero (and updates the manifest in place)
if the on-disk file was out of date. Stage the updated manifest and
re-commit; the second pass passes.

## Why pre-commit and not git hooks directly

Pre-commit handles the install, virtualenv resolution, and per-hook
isolation. It also makes the hook portable: contributors who clone
your repo get the same behaviour without manual setup. Toolr the
project uses pre-commit for the same reasons — see `.pre-commit-
config.yaml` at the repo root for the canonical example.
