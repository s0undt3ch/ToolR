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

### Breaking

- Linux release archives are now musl-only. The `x86_64-unknown-linux-gnu` and
  `aarch64-unknown-linux-gnu` binaries are no longer published — the musl
  builds are statically linked and run fine on glibc hosts, so shipping both
  only created ambiguity for downstream packaging (aqua-registry's Renovate
  updater disagreed with mise over which libc the plain `linux-x64` key
  should resolve to). If you need a gnu build, `cargo build --target
  x86_64-unknown-linux-gnu` still works from source; `installation/install.sh`
  now always resolves Linux hosts to the musl asset.

### Features

- `ctx.run(...)` gains an `interactive: bool = False` parameter. It inherits
  the real stdin/stdout/stderr instead of piping them, so a command that
  needs a real terminal — `$EDITOR`, `sops`, a login prompt — sees a TTY on
  all three streams. Previously the child's stdout/stderr were always piped
  regardless of `stream_output`/`capture_output`, so any command that gates
  on `isatty()` (full-screen editors especially) opened and immediately
  exited. `interactive=True` is incompatible with `capture_output`,
  `stream_output`, `no_output_timeout_secs`, and `input` — all four require
  piping. (#485)

### Fixes

- `DispatchCommand.argv` no longer emits a bare flag for a `nargs="+"`/`"*"`
  keyword argument that has no values. A `repeated` argument with an empty
  list in `command_args` used to reconstruct as a valueless flag — argparse
  rejects that outright for `"+"`, breaking any dispatched command with two
  or more `nargs="+"` keyword arguments (including mutually exclusive
  `nargs="+"` pairs, which were unusable entirely). The flag is now omitted
  for an empty list on both arities, since `command_args` can't distinguish
  "typed with zero values" from "never typed" for either one. (#483)
