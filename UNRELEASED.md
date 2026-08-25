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
