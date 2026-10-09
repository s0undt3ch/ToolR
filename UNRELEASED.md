<!--
UNRELEASED.md — Queued release notes for the next release.

Append narrative entries here as PRs land. On release, the
`_prepare-release.yml` workflow folds the content of this file
into the `### Notes` subsection of both the GitHub release body
and CHANGELOG.md (under the new version's heading), then resets
this file to empty for the next cycle.

Empty between releases is the steady-state — there's no header,
no scaffolding. Just write whatever should appear in the notes.
-->

Commands with a named positional parameter ahead of `*args` (for example
`def cmd(ctx, foo: Foo, *paths: pathlib.Path)`) now run. They used to crash with
`TypeError: ... got multiple values for argument`. Commands with positional-only
(`/`) parameters now work too: the manifest parser used to drop them, plus the
parameter right after them.

A Google-style `Args:` entry that wraps onto more than one line now shows in
full in `--help`. The parser used to keep only the first line. A continuation
line containing a colon (`see: the README`, a URL) also used to register a bogus
parameter, or overwrite a real one with the same name.

Running several `toolr` processes at once against the same project no longer prints
"failed to touch cache meta.json" warnings or leaves a truncated `meta.json` in the cache.

With `venv-location = "in-tree"`, toolr no longer reads all of `tools/.venv` on every
run to check whether the manifest is stale. It also no longer fails with
`hashing <repo>/tools: No such file or directory (os error 2)` when `uv sync` recreates
that venv while toolr commands are running. The freshness hash now skips `__pycache__`
and dot-directories under `tools/`, the same way the manifest parser already skipped
dot-directories.

Running several `toolr` commands in parallel (for example as pre-commit hooks) no longer fails at
random with `error: unrecognized subcommand`. Each process rewrote `tools/.toolr-manifest.json` in
place, so another process could read it half written, and toolr then quietly treated the manifest as
empty. The manifest is now replaced atomically, and a manifest toolr can't read is reported as an
error naming the file instead of hiding your commands.

When several `toolr` processes start at once against a stale or missing manifest
(for example parallel pre-commit hooks after editing `tools/*.py`), only one now
rebuilds it; the others wait for it and reuse the result. The lock lives in toolr's
cache directory, so nothing new appears in `tools/`. If the lock can't be taken, for
example on a read-only filesystem, the rebuild runs without it as before.
