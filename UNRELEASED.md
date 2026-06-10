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

### Security

- toolr no longer executes repository Python to build its command manifest.
  `toolr --help`, completion, and first-run are now fully static (AST parse +
  execution-free third-party glob). Repository code runs only on explicit
  command dispatch, through a provenance-verified interpreter. A committed
  `tools/.venv` is refused unless toolr provisioned it (`toolr project venv sync`).
- The toolr runner no longer puts the invocation directory on `sys.path` (the
  interpreter is started with `-P`), preventing a stray `.py` file in your
  current directory from shadowing stdlib/site-packages modules.

### Changed

- Commands now run with the working directory set to the repo root (like
  `make`/`cargo`). Relative path arguments resolve from the repo root, not your
  current directory; toolr prints a one-line note if you pass a relative path
  from a subdirectory.

### Removed

- The dynamic introspection layer (`toolr._introspect`) is gone. Commands
  registered dynamically (not via top-level `command_group(...)` + module-level
  `@group.command`) are no longer discovered. Third-party plugins via shipped
  `toolr-manifest.json` are unaffected.
