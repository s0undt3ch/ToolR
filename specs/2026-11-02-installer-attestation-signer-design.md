# Bind the installers' attestation check to the release workflow

- **Status:** approved
- **Origin:** whole-project review finding F-44, plus follow-up FU-11 (maintainer decision 2026-11-02).
- **Plan:** [`2026-11-02-installer-attestation-signer-plan.md`](./2026-11-02-installer-attestation-signer-plan.md)
- **Stacks on:** `fix/action-attestation-signer` (F-23, #646,
  `specs/archive/2026/2026-10-31-action-attestation-signer-design.md`). Merge that first.

## Problem

`installation/install.sh`, `installation/install.ps1` and the manual command in `docs/installation/index.md` all
verify a release archive with:

```sh
gh attestation verify <archive> --repo s0undt3ch/ToolR
```

This is the gap F-23 closed in the Setup ToolR action. `--repo` accepts an attestation from any workflow in the
repository, and `ci.yml` calls the same reusable `_build-binary-archive.yml` that releases use, with
`attestations: write`. So an archive built by CI on any branch passes, and so does another release's archive renamed
into place. The F-23 spec has the full reproduction: the signer is the same for CI and release builds, and only the
certificate's `buildConfigURI` (the top-level workflow) and the statement subject tell them apart.

FU-11 is a related gap on the mise path. mise's packslip backend checks who signed the packslip manifest, not which
workflow built the archives it lists. packslip links build provenance by digest, but neither `packslip verify` nor
mise checks it (<https://packslip.dev/docs/trust/>). Without a pin, mise accepts a manifest signed by any workflow of
the repository on first install and remembers that signer afterwards
(<https://mise.jdx.dev/dev-tools/backends/packslip.html#workflow>).

## Decision

### Installers

Both installers apply the action's rules unchanged:

- `--cert-identity` set exactly to `https://github.com/<repo>/.github/workflows/_build-binary-archive.yml@refs/heads/main`
  (exact SAN match, unlike the prefix match of `--signer-workflow`).
- `--deny-self-hosted-runners`.
- `--format json --jq` counting the verified attestations whose `buildConfigURI` is exactly
  `https://github.com/<repo>/.github/workflows/release.yml@refs/heads/main` and whose statement names a subject
  exactly like the downloaded archive.
- The count check fails closed on non-numeric output.
- `gh` 2.56 or later, checked before verifying, with a message naming the floor.

The `--jq` filter is the action's filter, byte for byte after whitespace is collapsed, and reads the two values from
the environment (`env.RELEASE_WORKFLOW`, `env.ARCHIVE_NAME`). gh runs jq itself, so neither installer needs `jq`.
Keeping the filter free of quotes and `$` also matters on Windows PowerShell 5.1, whose native-command argument
quoting mangles embedded double quotes.

The identities are built from the installer's repository setting (`TOOLR_REPO` / `-Repo`, default
`s0undt3ch/ToolR`), the same value already passed to `--repo` and used for the download URLs. With the default the
pins are byte-identical to the action's `SIGNER_WORKFLOW` and `RELEASE_WORKFLOW`. Someone who points the installer
at another repository already trusts that repository's releases; the override gets the same rules against that
repository instead of a guaranteed, confusing failure.

#### install.sh

The script is `#!/bin/sh` with `set -eu` and must stay POSIX: it runs under dash on Debian/Ubuntu, bash as `sh` on
macOS, and busybox `ash` on Alpine. The action's step only needs POSIX features (`${x##*/}`, `${v%%.*}`, `export`,
`[ "$count" -ge 1 ] 2>/dev/null`), so it ports nearly verbatim. There is no `pipefail`, so the gh call is written as
`if ! count=$(gh …); then err "attestation verification failed for $file"; fi` to keep the existing message; gh's
own reason still reaches stderr. shellcheck (run by prek) checks the `sh` dialect.

#### install.ps1

Supports Windows PowerShell 5.1 and PowerShell 7. The filter is passed as one line. `$env:RELEASE_WORKFLOW` and
`$env:ARCHIVE_NAME` are set for the call and restored afterwards, because `irm … | iex` runs in the user's session.
The native call doesn't redirect stderr: with `$ErrorActionPreference = "Stop"`, PowerShell 5.1 can turn redirected
native stderr into a terminating error. The script checks `$LASTEXITCODE`, then parses the count with
`[int]::TryParse` and fails unless it is at least 1. The version check parses `gh --version` with
`^gh version (\d+)\.(\d+)`.

#### Behaviour by mode

| Situation | `require` (default) | `auto` | `skip` |
| --- | --- | --- | --- |
| `gh` not on `PATH` | fail, existing message | warn and install, existing message | no check |
| `gh` older than 2.56, or version unreadable | fail: `gh >= 2.56 is required to verify attestations` | same failure | no check |
| no release attestation for this archive | fail: not built by the release workflow on main | same failure | no check |

`auto` keeps its meaning: verify when `gh` is installed, skip only when it isn't. An old `gh` already fails today
(gh before 2.56 can't load GitHub's current Sigstore trusted root, F-23 spec), so failing with a clear floor message
changes no outcome. Silently skipping instead would turn an outdated verifier into an unverified install.

The rejection message names the cause and the escape hatch (`--verify-attestation=skip` / `-VerifyAttestation skip`),
as the action's does.

### Manual command in the docs

`docs/installation/index.md` gets the full command with both identities and the archive name written into the `--jq`
filter, its expected output (`1`; `0` means "a valid ToolR attestation, but not a release build of this archive"),
and the gh 2.56 floor. It's a POSIX shell command; Windows users can run it from Git Bash or WSL, or let
`install.ps1` do the check.

### mise (FU-11)

`docs/installation/mise.md` recommends the per-tool option

```toml
"packslip:github.com/s0undt3ch/ToolR" = { version = "X.Y.Z", workflow = "release.yml@refs/heads/main" }
```

and says what it covers and what it doesn't:

- It makes mise accept only a packslip signed by `release.yml` running on `main` of `s0undt3ch/ToolR`, from the first
  install on, instead of whatever signer the first install saw. The match is exact.
- The `@refs/heads/main` suffix is required. mise expands a bare `workflow = "release.yml"` to the prefix
  `…/release.yml@refs/tags/`, and ToolR releases are signed on `main`, so every install fails.
- It doesn't check which workflow built the archives. The signed manifest pins each archive's digest and size, so
  the archives are the ones `release.yml` signed for, but their own build provenance is unchecked. For that, use the
  manual `gh attestation verify` command.
- It needs mise 2026.10.5 or later (the release that added `workflow`).

Checked with mise 2026.10.25 on 2026-11-02 against v0.35.1: `release.yml@refs/heads/main` installs;
`release.yml@refs/heads/mai` fails with `identity mismatch` (so the ref is matched exactly, not as a prefix);
`ci.yml@refs/heads/main` fails; bare `release.yml` fails with `expected an identity starting with
"…/release.yml@refs/tags/"`.

A test reads the documented pin from `mise.md`, expands it to the full identity and checks it equals
`publish-packslip`'s `--identity` in `release.yml`.

### Keeping the copies in step

The identities and filter now live in four places: `action.yml`, both installers and the docs. A test reads all of
them and asserts they agree, with the installers' default repository substituted. That's also the only check of
`install.ps1`'s identities and filter when `pwsh` isn't available.

## Rejected alternatives

- **The rejected alternatives in the F-23 spec** (`--signer-workflow …/release.yml`, `--source-ref`, attesting the
  archives from the release job too). They were rejected for the same reasons and apply here unchanged.
- **Hard-code `s0undt3ch/ToolR` in the installer identities.** Matches the action, but an installer pointed at
  another repository with `TOOLR_REPO` / `-Repo` would always fail verification. The download already trusts that
  repository.
- **Let `auto` skip on an old `gh`.** It would turn an outdated verifier into a silent unverified install, which is
  what `require`'s fail-closed default (SEC-08) is there to prevent. Old `gh` already fails today.
- **Ship a shared filter file the installers download.** `curl | sh` and `irm | iex` run a single file; fetching a
  second file at run time adds a trust problem. A test keeps the copies in step instead.
- **Recommend `identity = …` instead of `workflow`.** `identity` takes the full certificate identity and works too,
  but `workflow` is mise's documented form for this and is shorter. `workflow` can't be combined with `identity`.

## Scope and compatibility

- No manifest, runner schema or action change.
- Every published release (0.20.0 to 0.35.1, all built by `release.yml` on `main`) still installs through both
  installers. Archives built by any other workflow or ref, or attested for another file name, are rejected. Hosts
  verifying with `gh` need gh 2.56 or later, which they already needed in practice.
- Behaviour without `gh`, and with `--verify-attestation=skip` / `-VerifyAttestation skip`, is unchanged.
- `install-smoke.yml` already runs both installers with `require` on every PR that touches them, against the latest
  release, so CI checks the positive path on Linux, macOS and Windows.
- Skills: `skills/toolr-ci-setup` describes the action only. No skill text covers the installers or the mise pin, so
  no skill or reference change.
- Release note:

  > `install.sh`, `install.ps1` and the manual `gh attestation verify` command in the installation docs now only
  > accept a toolr archive whose attestation was produced by ToolR's release workflow on `main` for that exact
  > archive name, matching the Setup ToolR action. They used to accept an archive attested by any workflow in the
  > ToolR repository, including CI builds of unreleased branches, and a signed archive of another version or platform
  > renamed into place. Every published release still installs as before. Verifying an archive now needs gh 2.56 or
  > later. The mise docs now recommend pinning the packslip signer with
  > `workflow = "release.yml@refs/heads/main"` (mise 2026.10.5 or later).
