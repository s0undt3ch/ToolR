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

Automated dependency updates have moved from Dependabot to
[Mend Renovate](https://docs.renovatebot.com/). The new `renovate.json5`
preserves the previous ecosystem labels (`dependencies:rust`,
`dependencies:python`, `dependencies:github-actions`) and the weekly
Monday cadence, then cuts PR noise by grouping updates: the three
`ruff_*` crates (one `astral-sh/ruff` tag) ship together, every GitHub
Actions digest bump rolls into one PR per week, and the mise CLI tools
(`actionlint`, `shellcheck`, `prek`) share another. Language toolchain
pins (Python, uv, Rust, `cargo-edit`) and individual cargo / pyproject
crates still get their own PRs so each bump remains reviewable. GitHub
Actions stay pinned to commit SHAs with the SemVer tag in a trailing
comment.
