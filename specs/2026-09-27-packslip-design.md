# Publish a packslip with every toolr release

## Goal

Every toolr release from the next one onward ships a signed
[packslip](https://packslip.dev/release/v1/) bundle, so users can run:

```sh
mise use packslip:github.com/s0undt3ch/ToolR
```

The bundle declares:

- the five binary archives and the `toolr` executable inside each;
- the three agent skills in `skills/`, so `mise skills sync` can link
  the skills that match the installed toolr version;
- shell completions (bash, zsh, fish), so mise serves completions for
  the toolr version active in each project.

Earlier releases get no bundle. The aqua backend stays documented for
them.

## Non-goals

- No backfill of bundles for releases before the first packslip release.
- No trimmed skill archives. Skills ship from the repository as they are.
- No custom-domain hosting, release lists, or stampers.
- No change to the binary archive layout or to PyPI publishing.

## Facts this design rests on

These were checked on 2026-09-27 against v0.33.0 and packslip 1.3.0.

- An offline `packslip create --key … --no-log` over v0.33.0's five
  archives infers os, arch, and libc correctly for every triple
  (`linux/musl`, `darwin`, `windows`). With `bin = ["toolr"]` it
  resolves the executable through the `toolr-<ver>-<triple>/` top-level
  directory, including `toolr.exe`. No `[[artifact]]` overrides are
  needed.
- A manifest with only `bin` and `[[resource]]` entries works when the
  CLI supplies project, version, and commit.
- `toolr self completion print <shell>` exits 0 in an empty directory
  with an empty `HOME`, writes only to stdout, and creates no files. That
  meets packslip's exec rules (own directory, no stdin, stderr
  discarded).
- Each skill's frontmatter `name` matches its directory name.
- The repository does not enable immutable releases, so a job can
  upload to a release after it is published.
- The release also carries the `toolr-<ver>.tar.gz` sdist and
  `*.sha256` sidecars. A `toolr-*.tar.gz` glob would match the sdist.
- `jdx/packslip` puts its CLI on `GITHUB_PATH`, so later steps can call
  `packslip`. Tag `v1.3.0` is commit
  `4920350c39c234c7a969f9775040bd02829f1923`.

## Design

### 1. Shared manifest: `.github/packslip.toml`

One file holds everything that is the same for every release. The
release job and the PR gate both read it, so they cannot drift.

```toml
bin = ["toolr"]

[[resource]]
kind = "completion"
shells = ["bash", "zsh", "fish"]
exec = ["toolr", "self", "completion", "print", "{shell}"]

[[resource]]
kind = "skill"
name = "toolr-ci-setup"
repo = "skills/toolr-ci-setup"

[[resource]]
kind = "skill"
name = "toolr-command-authoring"
repo = "skills/toolr-command-authoring"

[[resource]]
kind = "skill"
name = "toolr-command-packaging"
repo = "skills/toolr-command-packaging"
```

It carries no project, version, source, or URL. Those come from action
inputs or CLI flags on each run.

Adding a skill under `skills/` means adding a `[[resource]]` entry here.
The PR gate (section 3) fails when the two disagree.

### 2. Release job: `publish-packslip` in `release.yml`

A new job, inline in `release.yml`. It must not move to a reusable
`_*.yml` workflow. Sigstore records the *called* workflow file as the
signing identity, and mise remembers that identity and asks users to
approve any change. The signer must stay
`https://github.com/s0undt3ch/ToolR/.github/workflows/release.yml@refs/heads/main`.

- `needs: [prepare-release, publish-release]`.
- `permissions: { contents: write, id-token: write }`. `attest: link`
  needs no `attestations: write`.
- Added to `set-pipeline-exit-status.needs`.

It is a separate job, not a step in `publish-release`, for these
reasons:

- When packslip fails, the GitHub release and PyPI publish have already
  finished. No release ends up half-published.
- "Re-run failed jobs" re-runs only this job. The job does not tag or
  push, so a re-run is safe.
- The job gets the smallest set of permissions.

The cost: if the job fails, the release has no bundle until someone
re-runs the job. mise does not see that version through packslip in the
meantime.

Steps:

1. `step-security/harden-runner`, egress audit, as in the other jobs.
2. `actions/checkout` at `ref: v<ver>`. This gives the job
   `.github/packslip.toml`.
3. Resolve the source commit:
   `git rev-parse "v${RELEASE_VERSION}^{commit}"`. Do not rely on
   `github.sha`. The workflow is `workflow_dispatch`, so `github.sha` is
   the commit *before* the release patch, not the tagged commit. Skill
   `repo:` sources are pinned to `source.commit`, so a wrong commit
   serves the wrong skills. `^{commit}` is required because the tag is
   annotated.
4. `jdx/packslip@4920350c39c234c7a969f9775040bd02829f1923 # v1.3.0`:
   - `tag: v<ver>`, `version: <ver>`, `commit: <resolved>`. The tag and
     version must be passed, because there is no triggering tag.
   - `manifest: .github/packslip.toml`.
   - `download:` with patterns that match only the binary archives:
     `toolr-*-*-apple-darwin.tar.gz toolr-*-*-linux-musl.tar.gz toolr-*-*-windows-msvc.zip`.
   - `attest: link`. `_build-binary-archive.yml` already attests each
     archive, so a second provenance statement adds nothing.
5. Post-publish check. The packslip docs say the action's local check
   does not replace this.
   - Download `packslip.sigstore.json` and the five archives from the
     published release with `gh release download`.
   - Run `packslip verify` with these flags:
     - `--identity https://github.com/s0undt3ch/ToolR/.github/workflows/release.yml@refs/heads/main`
     - `--issuer https://token.actions.githubusercontent.com`
     - one `--artifact` for each archive.
   - Run `packslip show` to check that `predicate.version` is `<ver>`
     and `predicate.source.commit` is the resolved commit.

### 3. PR gate: `packslip-check` job in `ci.yml`

This job catches archive-layout and manifest regressions on every PR,
before release day. It needs `build-binary-archive` and uses the three
runner-native archives that CI already builds. It signs nothing
publicly.

- Downloads the `toolr-archive-*` artifacts.
- Gets the CLI with `mise x github:jdx/packslip@1.3.0`. That version
  must match the action pin in section 2. A comment next to each pin
  names the other.
- Runs a throwaway `packslip keygen`, then
  `packslip create --manifest .github/packslip.toml --key … --no-log`
  with `--version 0.0.0-ci`, the current commit, and the archives.
- Runs `packslip verify --pubkey … --allow-unlogged`, with one
  `--artifact` for each archive.
- Checks the statement with `packslip show` and `jq`:
    - every artifact has exactly one `bin` entry ending in `/toolr` or
  `/toolr.exe`;
    - every Linux artifact has `libc: musl`;
    - the set of skill resource names equals the set of
  `skills/*/SKILL.md` directories, and each named directory has a
  `SKILL.md` whose frontmatter `name` matches.
- Added to `ci.yml`'s `set-pipeline-exit-status.needs`.

### 4. Skill self-containment

With a `repo:` source, a consumer gets only `skills/<name>/`. This work
depends on `archive/2026/2026-09-27-skills-self-contained-design.md`, which makes
each skill directory work on its own and adds a gate to keep it that
way. That work lands first, as the PR below this one in the stack.

### 5. Docs and release notes

- `README.md` and `docs/installation/mise.md` lead with
  `mise use packslip:github.com/s0undt3ch/ToolR`. They explain that
  mise verifies the signature against the release workflow's identity.
  aqua stays as the fallback for versions older than the first packslip
  release.
- `docs/skills.md` gains a "Via mise" section beside skillshare. It
  covers `mise skills ls`, `mise skills sync --dir .agents/skills`, and
  the opt-in `[settings.skills]` auto-sync. It repeats the packslip
  warning that auto-sync lets tool upgrades change what the agent reads.
  It also mentions version-matched completions through
  `mise completion <shell> --tool toolr`.
- `UNRELEASED.md` gets an entry for the new install path.
- `CHANGELOG.md` is not edited by hand.

## Error handling

| Failure | Where it shows | Recovery |
| --- | --- | --- |
| Archive layout or bin path changes | PR gate | Fix the layout or the manifest before merge. |
| Skill added without a manifest entry | PR gate | Add the `[[resource]]` entry. |
| Sigstore or Rekor outage at release | `publish-packslip` | Re-run the failed job. |
| Wrong commit or version in the bundle | `publish-packslip` post-publish check | Delete the bundle asset, fix, and re-run. |
| packslip action and CLI pins drift | Pins sit side by side with cross-reference comments | Bump both in one PR. |

## Testing

- The PR gate is the automated test for the manifest and archive layout.
- The OIDC signing path runs only in the release workflow. The first
  release after merge is its test, and the post-publish check verifies
  it.
- After that release, install it by hand with
  `mise use packslip:github.com/s0undt3ch/ToolR@<ver>` and
  `mise skills ls` to confirm that end to end.

## Risk noted, not addressed

mise 2026.9.14 fails to install packslip itself through its packslip
backend ("verified manifest project/version differs from discovery").
That is an upstream problem with packslip's own release, not with
consumers in general. If toolr hits the same error at the manual install
step, it goes upstream to jdx/packslip or jdx/mise.
