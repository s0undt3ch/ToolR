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

### `s0undt3ch/ToolR` GitHub Action: binary-only install, no more pipx

- **What changed:** the `Setup ToolR` composite action no longer
  installs toolr via `pipx install toolr==<version>`. The 0.20.0
  release shipped toolr as a binary-only PyPI wheel (no Python
  source), which pipx cannot install — so the old action path was
  already broken at the point this change landed.

  The rewritten action downloads the toolr binary archive directly
  from a GitHub release (`gh release download` with a `curl`
  fallback), cryptographically verifies the SLSA build provenance
  via `gh attestation verify`, caches the result, and puts the
  binary on `PATH`. It also caches `tools/.venv` keyed on
  `tools/pyproject.toml` + `tools/uv.lock` and sets
  `TOOLR_VENV_LOCATION=in-tree` so the cache works out of the box.

- **Minimum version:** the action refuses to install toolr below
  `0.20.0`. Earlier releases used the Python source distribution
  and are not compatible with the binary-only flow.

- **Migration:** remove the deprecated `python-path` and
  `requirements-file` inputs from your workflow's `Setup ToolR`
  step; the action has no use for them now that toolr is a
  standalone binary with its Python deps in the per-project
  `tools/.venv`. New optional inputs: `version` (defaults to the
  action ref, falling back to `latest`), `skip-attestation`
  (defaults to `false`), `cache-prefix` (defaults to `setup-toolr`),
  and `cache-tools-venv` (defaults to `true`).

### `installation/mise/` plugin: minimum toolr `0.20.0`, attests by default

- **What changed:** the bundled mise plugin under
  `installation/mise/` now rejects toolr versions below `0.20.0`
  (matching the action's cutoff) and verifies the SLSA build
  provenance via `gh attestation verify` on every install. Set
  `TOOLR_SKIP_ATTESTATION=1` to bypass — the plugin tells you so
  loudly if `gh` is missing from `PATH`.

## New features

### `TOOLR_VENV_LOCATION` environment variable

The new `TOOLR_VENV_LOCATION` env var overrides the
`[tool.toolr] venv-location` setting in `tools/pyproject.toml`.
Accepts the same `in-tree` / `cache` spellings the TOML key does.
Intended primarily for CI: the `Setup ToolR` action sets it to
`in-tree` automatically so workflows can cache `tools/.venv`
directly without forcing every consumer repo's
`tools/pyproject.toml` to declare `venv-location = "in-tree"`.

### Agent skills

Toolr now ships two in-tree agent skills, installable via
`skillshare` from this repository:

- **`toolr-command-authoring`** — teaches LLM coding assistants
  how to author toolr commands in a project's own `tools/*.py`
  files. Anchored on `toolr project init` and
  `toolr <group> <cmd> --help`; the API surface and docstring
  conventions are regenerated from `toolr-py`'s public surface
  and the parser's section-header table.
- **`toolr-command-packaging`** — teaches LLM coding assistants
  how to ship an existing set of toolr commands as a
  distributable Python plugin. Anchored on the in-tree
  `examples/plugin-package/`; the manifest fragment schema is
  regenerated from `toolr-core`'s serde types.

A new maintainer-only `crates/xtask/` workspace crate hosts the
generator (`cargo xtask build-skill-refs`). The `--check` variant
runs in CI on every PR (alongside the existing example-plugin
manifest check) so a public-surface change that forgets to
regenerate the skill references cannot land. A `prek` hook entry
gives the same gate locally.

See [docs/skills.md](https://toolr.readthedocs.io/latest/skills/)
for the user-facing installation flow.
