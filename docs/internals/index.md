# Internals

Reference material for contributors and curious users. None of this
is required reading for writing commands — head to
[Writing commands](../writing-commands/index.md) if that's what you want.

- [Manifest layers](manifest.md) — static + dynamic, hashing, rebuild
  semantics.
- [Cache layout](cache.md) — `$XDG_CACHE_HOME/toolr/`, `meta.json`,
  orphan/stale detection.
- [Pre-commit integration](pre-commit.md) — the shipped
  `.pre-commit-hooks.yaml`, what hooks toolr provides.
- [Diagnostics](diagnostics.md) — missing-dependency interception and
  the `toolr project venv sync` hint.

For the original design specs see
[`specs/archive/2026/rust-front-end/`](https://github.com/s0undt3ch/ToolR/tree/main/specs/archive/2026/rust-front-end)
in the repo.
