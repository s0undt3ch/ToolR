# Reject symlinked modules under `tools/` (F-17)

**Status:** Approved (2026-11-02)

## Problem

`list_python_files` in `crates/toolr-core/src/hash.rs` walks `tools/` with
`WalkDir::follow_links(false)` and keeps only entries whose own file type is a
regular file. A `tools/x.py` that is a symlink is dropped without a word: its
commands never appear in `toolr --help`, dispatch can't find them, and nothing
says why.

The same list feeds the static hash (`hash_tools_dir`, also run on every TAB
through `complete/freshness.rs`) and the static parser
(`parser/build.rs::build_commands`, also used for plugin fragments by
`toolr self build-manifest <pkg>`). `CommandsTester.discover()` mirrors the walk
in Python and skips every symlink too, so a test suite agrees with the silent
drop.

Repro:

```text
shared/linked.py
tools/demo.py
tools/linked.py -> ../shared/linked.py
```

`toolr --help` lists `demo` and silently omits `linked`.

Following the link instead would be worse: toolr would read, hash and (on
dispatch, or in `CommandsTester.discover()`) import code from wherever the
link points, possibly outside the repository, without anyone having asked
for it.

## Decision

toolr never reads or imports code from wherever a link points: no
surprises. A project's commands are the regular files under its `tools/`
directory. The supported way to share command code across projects or within
a monorepo is Python's own: put it in a package and depend on it from
`tools/pyproject.toml` (a path dependency works), or ship it as a toolr
plugin. Installed plugins are read only through their `toolr-manifest.json`
fragment, so toolr never walks from a user's repository into site-packages.
The same rule applies to plugin authors: `toolr self build-manifest` rejects
symlinked modules in a package by design.

Rejecting a link has to be loud. Silently dropping it hides commands.

- A `*.py` symlink anywhere the walk would list modules, dangling or not, is
  an error. Every offending path is reported at once, relative to the parent
  of the walked directory, with control and invisible format characters
  escaped as in other error text (`text::escape_control_chars`):

  ```text
  symlinked modules are not supported (2):
    - tools/linked.py
    - tools/sub/other.py
  Put the file itself under tools/, or share it as a Python package.
  ```

  For a plugin fragment the walked directory is the package, so the paths
  and the hint name the package directory instead of `tools`, even when it
  is given as `--source-dir .`.
- The error comes from `list_python_files` itself, so every caller reports it:
  `toolr --help`, dispatch (both the freshness hash and the rebuild),
  `toolr self build-manifest` and plugin fragment builds. Unlike other build
  errors, dispatch doesn't fall back to a cached manifest for it (a cache
  from an older toolr, or an untrusted repository's in-memory rebuild), since
  the cache would hide the links again. The walk executes nothing, so
  untrusted repositories get the error too.
- TAB completion stays silent. `run_complete` already falls back to an empty
  manifest when `resolve_manifest_at_tab` fails, so the shell is offered only
  the built-in `self` / `project` subtree, exit status 0, nothing on stderr.
- Symlinked directories keep their current behaviour: not followed, no error,
  including a directory link named `*.py` (only links pay for the stat that
  tells them apart; a dangling link still counts as a module). The walk
  already prunes dot-directories and `__pycache__`, so an in-tree
  `tools/.venv` that is itself a symlink, or a `*.py` link inside a pruned
  directory, never reaches the check.
- `CommandsTester.discover()` walks `tools/` the same way before importing
  anything and raises `ToolrError` with the same text, so a test suite can't
  go green on a tree toolr refuses. Both walks carry a "mirrors …; change both
  together" note.

The check costs nothing on the common path: `DirEntry::path_is_symlink()` and
`file_type()` come from the directory entry, so no entry is stat'd again.

## Rejected alternatives

- **Follow symlinks.** toolr would read and import code from outside
  `tools/`, possibly outside the repository, by surprise. Implemented once
  (kept outside `main`), it also needed rules for aliases of listed modules,
  links that resolve to the same file through different names, links under
  non-importable directories, dangling and looping links, and case-only
  aliases, mirrored in Rust and Python, to support a sharing mechanism Python
  packaging already provides.
- **Keep ignoring them silently.** The current behaviour: commands vanish and
  nobody is told why.

## Scope and compatibility

- Breaking for anyone with a symlinked `*.py` under `tools/`: toolr now
  refuses the tree instead of ignoring the file. The release note says so.
- Symlinked directories are unchanged.
- No manifest schema or runner-spec change.
- Docs: `docs/concepts.md` ("The `tools/` directory") states the rule;
  `docs/internals/manifest.md` stops saying every file under `tools/` feeds
  `static_hash` (only `*.py` files do).
