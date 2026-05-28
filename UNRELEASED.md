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

### `installation/mise/` plugin removed in favour of the aqua backend

- **What changed:** the bundled asdf-style mise plugin under
  `installation/mise/` has been deleted. mise users now install
  toolr through mise's built-in
  [aqua backend](https://mise.jdx.dev/dev-tools/backends/aqua.html)
  against the
  [aqua-registry entry](https://github.com/aquaproj/aqua-registry/tree/main/pkgs/s0undt3ch/ToolR):

  ```sh
  mise use aqua:s0undt3ch/ToolR@latest
  ```

  No plugin to register, no repository to clone, no `git::<url>//<subdir>`
  syntax to maintain. The aqua-registry entry pulls the same signed
  GitHub release archive the in-tree plugin used to fetch, so the
  installed binary is identical.

- **Why:** the in-tree plugin duplicated logic that mise already
  provides through the aqua backend (release-asset selection,
  SHA-256 verification, archive extraction). Owning that surface
  meant keeping the asdf-protocol hooks in sync with mise's
  evolving subdir-URL semantics and shipping our own attestation
  logic — work that aqua does once for everyone.

- **Migration:** replace the install command everywhere it appears
  (README, quickstart, mise docs page, internal runbooks, CI
  workflows). The old form

  ```sh
  mise plugin add toolr git::https://github.com/s0undt3ch/ToolR.git//installation/mise
  mise use --global toolr@latest
  ```

  becomes

  ```sh
  mise plugin uninstall toolr   # if previously installed
  mise use aqua:s0undt3ch/ToolR@latest
  ```

  (Add `--global` if you want toolr available across every
  directory; the project-scoped form above is the default we
  show in the headline docs.)

  The aqua-registry entry requires aqua-registry `v4.518.0` or
  newer (the first release that contains
  `pkgs/s0undt3ch/ToolR/registry.yaml`) and mise's aqua backend,
  which is built in and needs no extra configuration.
