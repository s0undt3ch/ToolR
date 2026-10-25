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

Bumped the bundled `rustls-webpki` to 0.103.16 to fix
[RUSTSEC-2026-0382](https://osv.dev/RUSTSEC-2026-0382), where certificate verification could
accept distrusted intermediates. This affects HTTPS downloads made by the `toolr` binary and
the `toolr-py` extension.
