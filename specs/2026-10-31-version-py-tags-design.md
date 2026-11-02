# Support pre-release versions in `tools/version.py` and the release workflow

- **Status:** design agreed (decided 2026-11-02), not implemented
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

## Decision

### Parsing and validation

`packaging` is already installed in the tools venv (a dependency of `toolr-py`). It becomes an explicit dependency in
`tools/pyproject.toml`, and `tools/uv.lock` is relocked.

- `_parse_version(raw: str) -> Version` strips one leading `v` (`removeprefix`, not `lstrip`) and parses the rest with
  `packaging.version.Version`. `InvalidVersion` becomes a `ValueError` naming the input.
- `_parse_release_version(raw: str) -> Version` calls `_parse_version`, then rejects anything Cargo's `X.Y.Z[-pre]`
  shape can't hold or that isn't a release: an epoch, a post-release (PEP 440 reads `1.0.0-1` as one), a dev release,
  a local segment, and a release segment that isn't exactly three numbers (`0.36`, `0.36.0.1`). Pre-releases
  (`a`, `b`, `rc`) are accepted.

`bump` validates an explicit `--new-version` first, before the tag check and before any write to `GITHUB_OUTPUT`,
`GITHUB_ENV` or `GITHUB_STEP_SUMMARY`. A `ValueError` is reported with `ctx.error` and `ctx.exit(1)`, not a
traceback. `--new-version v0.36.0` now behaves exactly like `--new-version 0.36.0`, so `--check-existing-tag` looks
for `v0.36.0`, not `vv0.36.0`.

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

The tag is parsed with `_parse_version`. A tag `packaging` can't parse is reported with `ctx.error` naming it, then
`ctx.exit(1)`.

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

`bump` also writes `release-prerelease=true|false` (from `Version.is_prerelease`) to `GITHUB_OUTPUT`.
`_prepare-release.yml` exposes it as a job and workflow output, and `release.yml`'s `Create GitHub release` step
passes `--prerelease` instead of `--latest` when it is `true`. A final release still passes `--latest`.

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
- Changes `tools/version.py`, `tools/pyproject.toml`, `tools/uv.lock`, `.github/workflows/_prepare-release.yml`,
  `.github/workflows/release.yml`, `tests/test_tools_version.py` and a new release workflow test under
  `tests/workflows/`. `action.yml` and the install scripts are unchanged.
- The release tooling ships nothing to users, but pre-release versions can now be released, so the change gets a
  short note in `release-notes/`.
- `--new-version 1.0.0rc1` and `--new-version 1.0.0-rc1` both release as `1.0.0-rc.1` (tag `v1.0.0-rc.1`).
