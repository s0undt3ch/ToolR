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
- [Diagnostics](diagnostics.md) — pre-flight + post-mortem missing-deps,
  `TOOLR_NO_PREFLIGHT_DEPS`.

For the original design specs see
[`specs/rust-front-end/`](https://github.com/s0undt3ch/ToolR/tree/main/specs/rust-front-end)
in the repo.
