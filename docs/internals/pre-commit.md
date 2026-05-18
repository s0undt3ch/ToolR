# Pre-commit integration

Toolr no longer ships a `.pre-commit-hooks.yaml`. The only hook it
used to provide — `toolr-manifest`, which rebuilt the manifest on
`tools/*.py` changes — only made sense when the manifest was tracked
in git. It isn't, anymore: `tools/.toolr-manifest.json` is a pure
cache that the binary auto-regenerates on hash drift, and toolr's
`project init` adds it to `tools/.gitignore` by default.

If your project chooses to commit the manifest anyway, wire your own
local hook:

```yaml
# .pre-commit-config.yaml
- repo: local
  hooks:
    - id: toolr-manifest
      name: Regenerate toolr manifest
      entry: toolr project manifest rebuild
      language: system
      pass_filenames: false
      files: ^tools/.*\.py$
```

That's the same definition toolr used to ship; it works identically
when invoked locally.

## Why the change

- The static manifest builds in ~10 ms on most projects; tab
  completion stays sub-50 ms regardless of staleness.
- The committed file caused merge conflicts on every PR that touched
  `tools/`, and the diffs weren't human-meaningful.
- The binary's hash-drift detector already keeps the on-disk file
  current when it is present.

See [Internals → Manifest layers](manifest.md) for the regeneration
flow.
