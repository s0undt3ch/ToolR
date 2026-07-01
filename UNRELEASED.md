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

The pre-commit configuration now sources its shared hooks (actionlint,
shellcheck, gitleaks, pin-github-actions, ruff, typos, rumdl, mypy, uv-lock)
from [`s0undt3ch/pre-commit-hooks`](https://github.com/s0undt3ch/pre-commit-hooks),
replacing the per-repo `rev:` pins and the local `.pre-commit-hooks/*.sh`
wrappers. Tool versions now have a single source of truth: binaries pinned in
`mise.toml` (Renovate-managed) and the venv `mypy` in the new `pre-commit` uv
dependency group. `typos` now auto-fixes on commit, and the redundant
`codespell` hook was dropped in favour of `typos` alone.

`typos` also now spell-checks commit messages (a `commit-msg`-stage hook), so
mistakes are caught before git-cliff folds commit subjects into the CHANGELOG —
which is itself no longer excluded from the check. Existing clones should re-run
`prek install --install-hooks` once to pick up the new `commit-msg` hook.
