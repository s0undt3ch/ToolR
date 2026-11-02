# Support pre-release versions in `tools/version.py` and the release workflow

- **Status:** implemented
- **Origin:** whole-project review finding F-26 (low).
- **Plan:** [`2026-10-31-version-py-tags-plan.md`](./2026-10-31-version-py-tags-plan.md)

## Problem

`toolr version bump` (`tools/version.py`) runs in every `Prepare Release` job: on each PR and push through `ci.yml`
(dev version) and on each release through `release.yml` (`--new-version`). It has two input-handling bugs.

**A pre-release tag breaks every dev build.** `_compute_dev_version` runs
`git describe --tags --long --match 'v[0-9]*.[0-9]*.[0-9]*'`. The glob's last `[0-9]*` matches any suffix, so a tag
like `v1.0.0-rc1` matches, and describe returns `v1.0.0-rc1-3-gabc1234`. Then:

```python
base, count, sha = describe.split("-")  # 4 parts: ValueError
```

Splitting from the right doesn't save it: `_bump_patch("1.0.0-rc1")` then fails on `int("0-rc1")`. Once one such tag
exists, every PR's `Prepare Release` job fails until it's deleted.

**`--new-version` is used before it's checked.** `bump` writes `release-version`, `release-patch-name` and
`RELEASE_VERSION` to `GITHUB_OUTPUT`/`GITHUB_ENV` before `cargo set-version` sees the value. With
`--new-version v0.36.0` (an easy slip, since the tags carry the `v`):

- `--check-existing-tag` looks for `vv0.36.0`, finds nothing, and passes.
- `release-version=v0.36.0` is written out.
- `cargo set-version` rejects it and the job fails, after the outputs were written. A malformed value like `0.36`
  takes the same path.

## Decided 2026-11-02

The original draft left one question open: should `--new-version` accept pre-releases such as `1.0.0-rc1`? The
maintainer's answer is yes, and that pre-releases are supported end to end, not only tolerated by the parser:

- Pre-release versions can be released (`1.0.0-rc1`, `1.0.0rc1`, `1.0.0-rc.1` all mean the same release).
- No hand-rolled version splitting. Versions are parsed and validated with `packaging.version.Version`.
- `git describe` output is no longer parsed. The tag, the commit count and the SHA come from three separate git
  commands, and the tag is parsed with `packaging`.
- A pre-release GitHub release is created with `--prerelease`, never `--latest`.
- The final release rolls up its pre-releases. An rc's GitHub release body shows everything queued so far, but an
  rc doesn't clear the queued release notes or prepend a CHANGELOG section, and git-cliff ignores pre-release tags,
  so the final release's notes and CHANGELOG cover everything since the last final release.

## Decision

### Parsing and validation

`packaging` is already installed in the tools venv (a dependency of `toolr-py`). It becomes an explicit dependency in
`tools/pyproject.toml`, and `tools/uv.lock` is relocked.

- `_parse_release_version(raw: str) -> Version` strips one leading `v` (`removeprefix`, not `lstrip`) and parses the
  rest with `packaging.version.Version`; `InvalidVersion` becomes a `ValueError` naming the input. It then rejects
  anything Cargo's `X.Y.Z[-pre]` shape can't hold or that isn't a release: an epoch, a post-release (PEP 440 reads
  `1.0.0-1` as one), a dev release, a local segment, and a release segment that isn't exactly three numbers (`0.36`,
  `0.36.0.1`). Pre-releases (`a`, `b`, `rc`) are accepted.

`bump` validates an explicit `--new-version` first, before the tag check and before any write to `GITHUB_OUTPUT`,
`GITHUB_ENV` or `GITHUB_STEP_SUMMARY`. A `ValueError` is reported with `ctx.error` and `ctx.exit(1)`, not a
traceback. `--new-version v0.36.0` now behaves exactly like `--new-version 0.36.0`, so `--check-existing-tag` looks
for `v0.36.0`, not `vv0.36.0`.

`--check-existing-tag` compares versions, not tag names. It lists `git tag --list 'v*'`, parses each tag with
`_parse_release_version` (skipping those that aren't release versions, such as the floating `v0.36`), and fails if
any equals the version being released. A hand-made `v1.0.0-rc1` or `v1.0.0rc1` therefore stops
`--new-version 1.0.0-rc1`, which would otherwise tag `v1.0.0-rc.1` as well and fail at the PyPI upload, after the tag
was pushed, because the wheel versions already exist.

### Rendering the Cargo version

`Cargo.toml` needs SemVer, so the parsed version is rendered from its parts by `_cargo_version(version) -> str`:

| PEP 440 (parsed) | Cargo (rendered) |
|---|---|
| `0.36.0` | `0.36.0` |
| `1.0.0rc1` | `1.0.0-rc.1` |
| `1.0.0a2` / `1.0.0b2` | `1.0.0-alpha.2` / `1.0.0-beta.2` |
| `1.0.1.dev3` | `1.0.1-dev3` |
| `1.0.0rc2.dev3` | `1.0.0-rc.2.dev3` |
| `1.0.1.dev3+gabc1234` | `1.0.1-dev3+gabc1234` |

`alpha`/`beta`/`rc` sort in that order under SemVer's ASCII comparison, as `a`/`b`/`rc` do under PEP 440. The
rendered string is what goes to `cargo set-version`, `action.yml`'s `inputs.version.default`, `release-version` (so
the tag is `v1.0.0-rc.1`), the archive names and the GitHub release title.

The wheels need PEP 440. maturin builds both wheels' versions from `Cargo.toml` and normalises them per PEP 440. A
probe with maturin 1.15.0 (`bindings = "bin"`, the binary wheel's shape) built `1.0.0-rc.2.dev5` as
`1.0.0rc2.dev5`, `1.0.0-rc.1` and `1.0.0-rc1` as `1.0.0rc1`, `1.0.1-dev3` as `1.0.1.dev3`, and
`1.0.1-dev3+gabc1234` as `1.0.1.dev3+gabc1234`. `packaging` applies the same normalisation, so the tests check
`str(Version(_cargo_version(v))) == str(v)`: every rendered Cargo version maps back to the PEP 440 version it came
from.

### Dev versions

`_compute_dev_version` runs:

1. `git describe --tags --abbrev=0 --match 'v[0-9]*.[0-9]*.[0-9]*'` for the latest tag. A non-zero exit (no
   matching tag) keeps the existing `TODAY_VERSION` fallback, counting with `git rev-list --count HEAD`.
2. `git rev-list --count <tag>..HEAD` for the number of commits since the tag.
3. `git rev-parse --short HEAD` for the SHA, used only on pull-request builds, as `+g<sha>` (the shape the
   `git describe` output gave until now).

`--abbrev=0` returns the nearest matching tag reachable from `HEAD`. Releases are cut from `main`, so on `main` and
on branches off it that is the latest release.

The tag is parsed with `_parse_release_version`. A tag that isn't a release version (`v1.2.3-foo`, `v1.2.3.4`) is
reported with `ctx.error` naming it, then `ctx.exit(1)`.

**Rule.** With `N` commits since the tag:

- After a final release `X.Y.Z`: `X.Y.(Z+1).devN`, Cargo `X.Y.(Z+1)-devN`. This is the current behaviour.
- After a pre-release `X.Y.Z{a,b,rc}K`: the next pre-release of the same kind, `X.Y.Z{a,b,rc}(K+1).devN`. After
  `v1.0.0-rc.1` (or `v1.0.0-rc1`) with 3 commits: `1.0.0rc2.dev3`, Cargo `1.0.0-rc.2.dev3`.
- Pull-request builds append `+g<sha>`.

Under PEP 440, which orders the published wheels, this is monotonic: `1.0.0rc1 < 1.0.0rc2.dev3 < 1.0.0rc2 < 1.0.0`,
and `1.0.0 < 1.0.1.dev3 < 1.0.1`. Under SemVer the dev version sorts above the tag and below the final release
(`1.0.0-rc.1 < 1.0.0-rc.2.dev3 < 1.0.0`), but above `1.0.0-rc.2`: SemVer ranks the longer pre-release identifier list
higher. That is harmless. Dev versions are never committed, and `cargo set-version`, which refuses to lower the
version, compares against the released version in `Cargo.toml` (`1.0.0-rc.1`), which the dev version is above.

### Release outputs and the GitHub release

`bump` also writes `release-prerelease=true|false` to `GITHUB_OUTPUT`: `true` when the version has a pre-release
segment. `Version.is_prerelease` isn't used, because it is also true for dev releases, and a dev build after a final
release must report `false` so CI dry runs keep exercising the steps a final release runs. `_prepare-release.yml`
exposes it as a job and workflow output, and `release.yml`'s `Create GitHub release` step passes `--prerelease`
instead of `--latest` when it is `true`. A final release still passes `--latest`.

### Final releases roll up pre-releases

Decided 2026-11-02 by the maintainer. In `_prepare-release.yml`, an rc:

- still loads the queued release notes and renders the GitHub release body with `git-cliff --unreleased`, so the rc's
  release shows everything queued since the last final release;
- skips `Update CHANGELOG.md` (`git-cliff --unreleased --tag … --prepend CHANGELOG.md`) and
  `Clear queued release notes`. Both steps carry `if: steps.bump-version.outputs.release-prerelease != 'true'`.

`cliff.toml` sets `tag_pattern = "^v[0-9]+\.[0-9]+\.[0-9]+$"`, so git-cliff only treats final release tags as
releases. Checked with git-cliff 2.13.1 in a scratch repository tagged `v0.35.1`, `v1.0.0-rc.1`, then one more commit:

- `ignore_tags` is the wrong option. It folds an rc's commits into the next section of a full changelog, but
  `--unreleased` still starts after the ignored tag, so the final release's `--unreleased --tag 1.0.0` section (what
  the workflow prepends) lost the commits before the rc.
- `skip_tags` drops the rc's commits entirely.
- With `tag_pattern`, `--unreleased --tag 1.0.0` lists the commits from before and after the rc, and a full changelog
  has no `1.0.0-rc.1` section.

The skip parser for release commits was `^chore\(release\): prepare for`, but the release commit is
`chore(release): Prepare for X release`, so it never matched. Before the rollup the commit sat on the tag and fell
into its own section; with rc tags out of the pattern, each rc's release commit would land in the final release's
section and in later rc bodies. The parser is now case-insensitive (`(?i)`).

git-cliff is pinned (2.14.2) in `mise.toml`, so CI's test jobs have it, and `_prepare-release.yml` runs that
mise-installed git-cliff directly instead of `orhun/git-cliff-action`, which installed its own copy and would have
needed a second pin that Renovate doesn't bump. The run steps pass the `GITHUB_TOKEN` and `GITHUB_REPO` the action
provided for the template's contributor lookup. The git-cliff tests fail instead of skipping when `CI` is set and
git-cliff is missing.

`_compute_dev_version` returns a PEP 440 `Version`, and `bump` renders the Cargo form once. The pre-release policy is
documented for contributors in "Pre-releases" in `release-notes/README.md`, which names the four files that implement
it.

On this repository's history the pattern changes nothing: `--unreleased --tag 0.36.0` renders the same with it as with
the previous config limited to `v*` tags. The pattern also leaves out the floating `vX`/`vX.Y` tags and the old
unprefixed `0.0.0` tag, which were never release sections that `--unreleased` should start from.

The "latest" lookups need no change. GitHub's latest-release endpoint (`GET /repos/{owner}/{repo}/releases/latest`)
returns "the most recent non-prerelease, non-draft release", and a release flagged as a pre-release can't be marked
latest. All three lookups go through it:

- `action.yml`'s `resolve-version` step runs `gh release view` with no tag, which views the latest release.
- `installation/install.ps1` calls `/releases/latest` on the REST API.
- `installation/install.sh` follows the `https://github.com/<repo>/releases/latest` redirect, which resolves the
  same release.

That contract is GitHub's, so it isn't tested locally; a stubbed `gh` would only test the stub. What is tested is the
part this repository controls: that a pre-release is created with `--prerelease` and without `--latest`.

An rc release bakes `1.0.0-rc.1` into `action.yml`'s `inputs.version.default` on `main`, the same as a final release
does. That is the bake-in's purpose: a caller pinning the action at that commit gets the matching binary.

Rejected alternatives:

- **One regex over `git describe` output.** The first draft of this design. It works, but it is the hand-rolled
  splitting the decision rules out, and it can't express the pre-release dev rule without more of it.
- **`--exclude 'v*-*'` on `git describe`.** It skips rc tags, so the dev version derives from the last final release
  (`0.36.1.devN`), which is lower than a `1.0.0-rc.1` already in `Cargo.toml`: `cargo set-version` would refuse it.
- **Passing the PEP 440 form to Cargo.** `cargo set-version` rejects `1.0.0rc1`; it isn't SemVer.
- **Rejecting a leading `v`.** The action's `version` input accepts both forms; accepting both here removes the slip.

## Scope and compatibility

- No runner or manifest schema change.
- Changes `tools/version.py`, `tools/pyproject.toml`, `tools/uv.lock`, `cliff.toml`, `mise.toml`, `mise.lock`,
  `tests/distribution/test_cross_wheel.py`,
  `.github/workflows/_prepare-release.yml`, `.github/workflows/release.yml`, `tests/test_tools_version.py` and two
  new workflow tests under `tests/workflows/` (`test_release_create.py`, `test_prepare_release_prerelease.py`; the
  git-cliff rollup cases skip when `git-cliff` isn't on `PATH`, and a config assertion pins `tag_pattern` either way).
  `action.yml` and the install scripts are unchanged.
- `tests/distribution/test_cross_wheel.py` compared the wheel version with `toolr --version` as strings, mapping only
  `.dev` to `-dev`, so a pre-release wheel (`1.0.0rc1`) never matched the binary's `toolr 1.0.0-rc.1`. It now parses
  both with `packaging`, and unit tests outside the distribution suite pin the comparison. The other version checks
  compare like with like: the mise smoke tests compare the binary with the tag's Cargo form, and the install smoke's
  `pip install toolr==<version>` normalises the Cargo form itself.
- The release tooling ships nothing to users, but pre-release versions can now be released, so the change gets a
  short note in `release-notes/`.
- `--new-version 1.0.0rc1` and `--new-version 1.0.0-rc1` both release as `1.0.0-rc.1` (tag `v1.0.0-rc.1`).
