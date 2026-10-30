# Escape filesystem paths before they become glob patterns

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-07 (high).
- **Plan:** [`2026-10-30-plugin-glob-escape-plan.md`](./2026-10-30-plugin-glob-escape-plan.md)

## Problem

Three call sites build a glob pattern by joining a real filesystem path with a
wildcard tail, then hand the whole string to `glob::glob`. Any `[`, `]`, `*`
or `?` in the path becomes pattern syntax:

| Call site | Path that leaks into the pattern | Effect |
|---|---|---|
| `crates/toolr-core/src/third_party/glob.rs` `glob_manifests` | the tools venv | plugin fragments not found |
| `crates/toolr-core/src/argparse/scan.rs` `scan_block_paths` | the project root | argparse `scan_paths` match nothing |
| `crates/toolr/src/build_manifest_resolve.rs` `find_in_venv` | the venv and the package name | `self build-manifest <pkg>` can't find the package |

The first is the worst. `compute_third_party_hash` (`manifest_build/hash.rs`)
uses the same `glob_manifests`, so it hashes the same empty set every time.
The manifest never goes stale and nothing warns. A repo checked out under
`~/src/[wip]/repo` with `venv-location = "in-tree"` silently loses every
plugin command. A user-set `TOOLR_VENV_LOCATION` or cache path containing
these characters does the same.

Repro: the same fixture, plugin installed, under `…/repo` and
`…/[wip]/repo`; `toolr project manifest rebuild` reports 2 groups /
2 commands for the first and 1 / 1 for the second.

## Decision

Escape the literal part of every pattern with `glob::Pattern::escape` before
appending the wildcard tail. The tail stays a pattern.

- `glob_manifests`: escape `tools_venv`, then append
  `lib/python*/site-packages/*/toolr-manifest.json` (and the `Lib` variant).
- `scan_block_paths`: escape `root`, then append the user's `scan_paths`
  entry unescaped. `scan_paths` is documented as a glob, so it stays one.
- `find_in_venv`: escape `venv_dir` and `package`. A package name is a
  literal, never a pattern.

Build the pattern as a string from the escaped prefix plus `/`-joined
segments rather than `Path::join` on the escaped string. `Pattern::escape`
wraps metacharacters as `[[]`, `[]]`, `[*]`, `[?]`, which are valid path
characters, so `Path::join` works too. Either is fine, as long as the
escaped prefix is never re-parsed as a path for anything but globbing.

Windows: `glob` treats `\` as a separator there, not an escape, and
`Pattern::escape` uses bracket classes, so the same code works.

## Scope and compatibility

- No manifest or runner schema change.
- `third_party_hash` changes for affected repos only (it now covers the
  fragments it always should have). The manifest rebuilds once. That's the
  fix working.
- Not covered: F-14's Windows `Lib/` layout gap in `find_in_venv`. That
  needs its own PR.
- `UNRELEASED.md`:

  > Plugins installed in a tools venv whose path contains `[`, `]`, `*` or
  > `?` (for example a repo checked out under `~/src/[wip]/`) are found
  > again. toolr used to treat those characters as glob syntax, so it
  > dropped the plugin's commands without a warning. The same fix applies
  > to argparse `scan_paths` under such a project root and to
  > `toolr self build-manifest <package>`.
