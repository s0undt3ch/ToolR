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

## ⚠ Breaking changes

### `toolr project deps` removed; replaced by `toolr project venv`

- **What changed:** the `toolr project deps` subcommand group has
  been removed. Its two commands moved under `toolr project venv`:
    - `toolr project deps sync` → `toolr project venv sync`
    - `toolr project deps upgrade <pkg>` → `toolr project venv upgrade <pkg>`
- **Behavior change on `sync`:** the new `toolr project venv sync`
  honours the tools venv's freshness stamp by default and no-ops
  (exit 0, no `uv sync`) when the venv is already up to date.
  Use `--force` to re-run unconditionally — that matches what
  `toolr project deps sync` did before.
- **New `--quiet` flag on `sync`:** silent on success and on
  benign unattended-mode exits ("not a toolr repo", "lock missing",
  "uv install needs consent"). Designed for use from a mise
  `[hooks].enter` recipe — see
  [Auto-sync the tools venv on shell-enter](https://toolr.readthedocs.io/latest/installation/mise/#auto-sync-the-tools-venv-on-shell-enter).
- **Migration:** running `toolr project deps <anything>` at 0.22
  prints a tailored error pointing at the new path and exits with
  code 2.
- **Why:** the `deps` group only ever held venv-touching operations;
  collapsing it under `venv` puts every tools-venv operation in one
  place and makes room for future uv-wrapper subcommands (`add`,
  `remove`, `lock`, …) — see
  [#288](https://github.com/s0undt3ch/ToolR/issues/288) — to land in
  the obvious location.
