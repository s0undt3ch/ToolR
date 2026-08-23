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

**BREAKING (GitHub Actions consumers):** removed the rolling-tag sync mechanism
(`sync-rolling-tags.yml`, `toolr ci sync-rolling-tags`) that force-pushed floating `latest`/`vX`/`vX.Y`
git tags on release. Those tags still exist but will no longer move — anyone with `uses:
s0undt3ch/ToolR@latest`, `@v1`, or `@v1.2`-style pins is now frozen on whatever commit last synced,
with no error to signal it. Pin to an explicit released tag or commit SHA instead (the recommended
form has always been `uses: s0undt3ch/ToolR@<sha> # vX.Y.Z`, per the `toolr-ci-setup` skill).
