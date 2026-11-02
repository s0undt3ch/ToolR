# Bind the action's attestation check to the release workflow

- **Status:** implemented
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
# step env:
#   SIGNER_WORKFLOW: https://github.com/s0undt3ch/ToolR/.github/workflows/_build-binary-archive.yml@refs/heads/main
#   RELEASE_WORKFLOW: https://github.com/s0undt3ch/ToolR/.github/workflows/release.yml@refs/heads/main
ARCHIVE_NAME="${ARCHIVE_PATH##*/}"; export ARCHIVE_NAME RELEASE_WORKFLOW
count=$(gh attestation verify "$ARCHIVE_PATH" \
  --repo s0undt3ch/ToolR \
  --cert-identity "$SIGNER_WORKFLOW" \
  --deny-self-hosted-runners \
  --format json \
  --jq '[.[].verificationResult
         | select(.signature.certificate.buildConfigURI == env.RELEASE_WORKFLOW
                  and any(.statement.subject[]; .name == env.ARCHIVE_NAME))]
        | length')
if ! [ "$count" -ge 1 ] 2>/dev/null; then echo "setup-toolr: … was not built by the ToolR release workflow on main" >&2; exit 1; fi
```

The step first checks `gh --version` and fails with `setup-toolr: gh >= 2.56 is required to verify attestations`.
`--format json` arrived in gh 2.51, and the official gh 2.54.0 and 2.55.0 binaries fail on
every ToolR archive with `failed to get trusted root: unsupported tlog public key type: PKIX_ED25519`. gh 2.56.0 is
the oldest release that verifies a ToolR archive (run on 2026-11-02 against 0.35.1 and 0.20.0). The version is
compared as major and minor integers in bash, not with `sort -V`, so Windows runners can't pick up `sort.exe`.

The count check fails closed: `[ "$count" -ge 1 ]` errors on non-numeric output, and the negated test rejects
that too.

The `statement.subject[].name` match binds the attestation to the archive the action asked for. Without it, any
older or other-triple release archive, which carries its own valid release attestation, could be swapped in under
the requested name. On 2026-11-02, v0.34.0's x86_64 musl archive renamed to the 0.35.1 name verified under `gh`
but the filter gave `0`.

- `--cert-identity` and `--deny-self-hosted-runners` are enforced by `gh` itself. All archive builders are GitHub-hosted (`tools/ci.py`, binary-archive
  targets), and both checked releases pass with these flags.
- The `buildConfigURI` filter is the part that rejects CI builds. `gh` has no flag for it, so the step reads it from
  the JSON of attestations that already passed verification. The field is a Fulcio certificate extension, signed
  like the rest of the certificate. On the repro above it gives `1` for 0.35.1 and 0.20.0 and `0` for the CI
  artifact.
- Both the signer and `buildConfigURI` are pinned to `refs/heads/main`, by exact match. The `release` environment and
  the permission check live in `release.yml` itself, so anyone who can push a branch can commit an edited copy
  without them and get an archive attested as `release.yml@refs/heads/<branch>` (or `@refs/pull/<n>/merge`). An
  unpinned `release.yml@` prefix would accept that. All published releases, 0.20.0 to 0.35.1, were built from `main`
  and pass the pinned check. A future maintenance-branch release would need this pin widened deliberately.
- The signer is pinned with `--cert-identity`, an exact match on the certificate SAN. `--signer-workflow …@refs/heads/main`
  is matched by `gh` as a prefix, so it would also accept `@refs/heads/main-evil`. Round 3 of review replaced an
  earlier `--signer-workflow` plus an exact SAN check in `--jq` with this; checked with gh 2.56.0 and 2.103.2.
- Each identity is written once, as `SIGNER_WORKFLOW` and `RELEASE_WORKFLOW` in the step's `env:`.
  `tests/action_yml/test_verify_attestation.py` checks that both workflow files exist, that `release.yml` calls the
  signer, and that `publish-packslip`'s `--identity` is the same `release.yml` identity.
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
- Every published release (0.20.0 to 0.35.1, all built by `release.yml` on `main`) still verifies. Archives built by
  any other workflow, or by `release.yml` on any other ref, are rejected; there is no supported way to install those
  through the action. Runners need gh 2.56 or later.
- Assumes the parent branches changed only the `resolve-version` step, the `version` input description and the
  `venv-cache` step. This change touches only the "Verify SLSA build provenance" step and `SKILL.md`.
- Skill: `skills/toolr-ci-setup/SKILL.md` "Common failure modes" gets a bullet for the new rejection
  (`skills/toolr-ci-setup/REVIEW.md` asks for this when attestation behaviour changes). Input descriptions don't
  change, so `references/action.md` doesn't regenerate.
- Not covered, same gap, separate follow-up: `installation/install.sh`, `installation/install.ps1` and the manual
  `gh attestation verify … --repo s0undt3ch/ToolR` command in `docs/installation/index.md`.
- Release note:

  > The Setup ToolR action now only accepts a toolr archive whose attestation was produced by ToolR's release
  > workflow on `main` for that exact archive name. It used to accept an archive attested by any workflow in the
  > ToolR repository, including CI builds of unreleased branches, and a signed archive of another version or
  > platform renamed into place. Every published release still installs as before. Verifying an archive now needs
  > gh 2.56 or later on the runner.
