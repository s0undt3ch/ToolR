# Bind the action's attestation check to the release workflow

- **Status:** design agreed, not implemented
- **Origin:** whole-project review finding F-23 (low, reported as plausible; reproduced below).
- **Plan:** [`2026-10-31-action-attestation-signer-plan.md`](./2026-10-31-action-attestation-signer-plan.md)
- **Stacks on:** `fix/action-cache-key` (F-39, `specs/2026-10-31-action-cache-key-design.md`), which stacks on
  `fix/action-latest-repo` (F-22); merge those first.

## Problem

The Setup ToolR action's "Verify SLSA build provenance" step (`action.yml`) runs:

```sh
gh attestation verify "$ARCHIVE_PATH" --repo s0undt3ch/ToolR
```

`--repo` only requires that some workflow in `s0undt3ch/ToolR` signed the archive. Release archives are attested by
the reusable `.github/workflows/_build-binary-archive.yml`, and `ci.yml` calls that same workflow on every push and
same-repo PR, with `id-token: write` and `attestations: write`. So any CI build, from any branch, produces an
attestation the action accepts. Anyone who can push a branch can get an arbitrary archive attested; combined with a
way to swap a release asset, the action's supply-chain gate passes.

Reproduced on 2026-10-31 with `gh` 2.103.2:

- The `toolr-0.35.2-dev114-x86_64-unknown-linux-musl.tar.gz` artifact from `ci.yml` run 41126624626 (push to
  `main`) passes the action's current command (exit 0).
- Its certificate: signer (`subjectAlternativeName`) `_build-binary-archive.yml@refs/heads/main`, `buildConfigURI`
  `ci.yml@refs/heads/main`, trigger `push`.
- The 0.35.1 and 0.20.0 release archives: signer `_build-binary-archive.yml@refs/heads/main`, `buildConfigURI`
  `release.yml@refs/heads/main`, trigger `workflow_dispatch`.

Signer workflow and source ref are the same for both. Only `buildConfigURI`, the top-level workflow that called the
reusable one, tells a release build from a CI build.

The audit's suggested fix, `--signer-workflow s0undt3ch/ToolR/.github/workflows/release.yml`, fails on every real
release (`Error: verifying with issuer "sigstore.dev"` on 0.35.1): the signer is the reusable workflow, not
`release.yml`. The release job's own `actions/attest` step covers `dist/*`, and the archives are downloaded to
`release-assets/`, so `release.yml` never signs an archive itself.

## Decision

Verify in two layers, both in the existing step:

```sh
count=$(gh attestation verify "$ARCHIVE_PATH" \
  --repo s0undt3ch/ToolR \
  --signer-workflow s0undt3ch/ToolR/.github/workflows/_build-binary-archive.yml \
  --deny-self-hosted-runners \
  --format json \
  --jq '[.[].verificationResult.signature.certificate.buildConfigURI
         | select(startswith("https://github.com/s0undt3ch/ToolR/.github/workflows/release.yml@"))] | length')
[ "$count" -ge 1 ] || { echo "setup-toolr: … was not built by the ToolR release workflow" >&2; exit 1; }
```

- `--signer-workflow` and `--deny-self-hosted-runners` are enforced by `gh` itself. They follow the flags `ci.yml`
  already uses to verify the packslip CLI. All archive builders are GitHub-hosted (`tools/ci.py`, binary-archive
  targets), and both checked releases pass with these flags.
- The `buildConfigURI` filter is the part that rejects CI builds. `gh` has no flag for it, so the step reads it from
  the JSON of attestations that already passed verification. The field is a Fulcio certificate extension, signed
  like the rest of the certificate. On the repro above it gives `1` for 0.35.1 and 0.20.0 and `0` for the CI
  artifact.
- The filter pins `release.yml@` without a ref. Releases are dispatched from `main` today, but `release.yml` is gated
  by the `release` environment and a permission check, not by branch. Pinning `@refs/heads/main` would break a future
  maintenance-branch release for no gain.
- `jq` runs inside `gh` (`--jq`), so no extra tool is needed on the runner.
- The step gets `id: verify-attestation` so tests can extract it.
- The failure message names the cause and the escape hatch (`skip-attestation: true`), as the other errors do.

### Rejected alternatives

- **`--signer-workflow …/release.yml`** (the audit's fix). Fails on every existing release; see Problem.
- **Have the release job attest the archives too** (add `release-assets/*` to its `subject-path`), then use
  `--signer-workflow …/release.yml`. Clean going forward, but every release before the change lacks that attestation,
  so the action would need a version gate and a second code path for older `version:` pins. The `buildConfigURI`
  filter works for every release since 0.20.0 with one code path. Worth doing later as extra hardening; not needed
  for this fix.
- **`--source-ref`.** CI pushes to `main` and releases both have `refs/heads/main`, so it doesn't separate them.
- **`--signer-workflow …/_build-binary-archive.yml` alone.** Same signer for CI and release builds; doesn't close
  the gap.

## Scope and compatibility

- No manifest or runner schema change.
- Every published release (0.20.0 and later, built by `release.yml`) still verifies. Archives built by any other
  workflow are rejected; there is no supported way to install those through the action.
- Assumes the parent branches changed only the `resolve-version` step, the `version` input description and the
  `venv-cache` step. This change touches only the "Verify SLSA build provenance" step and `SKILL.md`.
- Skill: `skills/toolr-ci-setup/SKILL.md` "Common failure modes" gets a bullet for the new rejection
  (`skills/toolr-ci-setup/REVIEW.md` asks for this when attestation behaviour changes). Input descriptions don't
  change, so `references/action.md` doesn't regenerate.
- Not covered, same gap, separate follow-up: `installation/install.sh`, `installation/install.ps1` and the manual
  `gh attestation verify … --repo s0undt3ch/ToolR` command in `docs/installation/index.md`.
- `UNRELEASED.md`:

  > The Setup ToolR action now only accepts toolr archives built by ToolR's release workflow. It used to accept an
  > archive attested by any workflow in the ToolR repository, including CI builds of unreleased branches. Every
  > published release still installs as before.
