# uv install artefact verification: design

Audit finding F-06 (medium, security).

## Problem

The opt-in uv auto-install trusts whatever bytes `github.com` serves. TLS is
its only control. On `main` (`a14ce9a9`), in `crates/toolr-core/src/uv/install.rs`:

- `asset_url` (lines 253–255) builds
  `https://github.com/astral-sh/uv/releases/latest/download/<asset>.<ext>`.
- `perform_install` (lines 258–271) picks the host asset and calls
  `download_and_extract`.
- `download_and_extract` (lines 273–314) does `reqwest::blocking::get`, reads
  the body, unpacks it, copies `uv` to `$XDG_DATA_HOME/toolr/bin/uv` and marks
  it `0o755`. Nothing hashes the download.

Callers: `ensure_uv` (`uv/mod.rs:224`), reached after the consent prompt or
`TOOLR_AUTO_INSTALL_UV=1`, and the hidden `toolr __install-uv-now`
(`crates/toolr/src/dispatch.rs:233`). The base URL is hard-coded. No
environment variable points the installer at a mirror.

## Constraints

- No pinned uv version and no pinned checksum table in toolr. A new uv release
  must never need a toolr release.
- New `toolr-core` dependencies are `optional = true` and listed under `full`.

## What uv publishes

Checked against uv `0.13.7` (2026-11-03):

- Each archive has a `<asset>.sha256` sidecar in the same release, in
  `sha256sum` format: `<64 hex>  <asset file name>`. A combined `sha256.sum` and
  a `dist-manifest.json` ship too.
- The release is a GitHub **immutable release** (`"immutable": true` in the
  releases API). Its assets can't be replaced once published, so an asset and
  the sidecar under the same tag stay paired.
- Each archive has two GitHub attestations: SLSA provenance v1 from
  `astral-sh/uv`'s `.github/workflows/release.yml` on `refs/heads/main`, and a
  release attestation.
- The releases API also reports a GitHub-computed `digest` for each asset.
- `/releases/latest/download/<file>` answers `302` to
  `/releases/download/<tag>/<file>`, which answers `302` again to a signed
  `release-assets.githubusercontent.com` URL.

## Design

### Resolve the tag once

`/latest` can move between two requests. If toolr fetched the archive and the
sidecar from `/latest` separately, a release published in between would pair
an archive with the wrong checksum. The installer resolves the tag first:

1. `GET <base>/releases/latest/download/<asset>.<ext>` with a client that
   doesn't follow redirects. Require a 3xx with a `Location` whose path ends in
   `/releases/download/<tag>/<asset>.<ext>`. `<tag>` must be one path segment of
   `[0-9A-Za-z._+-]`, and neither `.` nor `..`. Anything else fails.
2. Fetch `<base>/releases/download/<tag>/<asset>.<ext>.sha256` and
   `<base>/releases/download/<tag>/<asset>.<ext>` with the normal client, which
   follows the CDN redirect.

`<base>` is `https://github.com/astral-sh/uv`. All three requests go to
`github.com` web routes and its asset CDN, so the 60-requests-an-hour limit on
the unauthenticated REST API doesn't apply.

### Verify before unpacking

- Parse the sidecar: the first whitespace-separated token must be 64 hex
  digits. If a second token is present, it must equal `<asset>.<ext>`, with a
  leading `*` (binary-mode marker) stripped. Anything else is malformed.
- Compute SHA-256 over the downloaded bytes in memory, before anything touches
  disk. Compare it to the sidecar digest, ignoring case.
- On a match, unpack and install as today. On any failure, nothing is written
  under `$XDG_DATA_HOME/toolr/bin`.

Hashing uses the `sha2` crate, added as an optional dependency under `full`.

### Fail closed

Every verification failure stops the install with an error:

- the tag can't be resolved (no 3xx, no `Location`, an unexpected path, a bad
  tag);
- the sidecar can't be fetched (network error or non-2xx);
- the sidecar is malformed or names another file;
- the digest doesn't match.

There's no bypass variable. The escape hatch already exists: install uv by
another route and toolr uses the copy on `PATH`. Offline and air-gapped hosts
are unchanged, because auto-install already needs the network.

### Errors

New `UvError` variants:

- `Unverified { asset: String, reason: String }`: tag resolution failed, or
  the sidecar was missing or malformed.
- `ChecksumMismatch { asset: String, tag: String, expected: String, actual: String }`.

Display text names the asset, the tag and both digests. `user_message` adds
the advice: "toolr didn't install uv because it couldn't verify the download.
Install uv yourself from <https://docs.astral.sh/uv/getting-started/installation/>
and rerun."

### Proxies and mirrors

`reqwest` reads `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY` by default, so a proxy
works on all three requests. The no-redirect client is built with
`reqwest::blocking::Client::builder()`, which keeps that default. Mirror
support stays out of scope (open question 2).

### Test seam

`perform_install()` keeps its signature and calls a new
`pub(crate) fn perform_install_from(base: &str, dest: &Path) -> Result<UvBinary, UvError>`,
passing the GitHub constant and `managed_uv_path()`. Tests call
`perform_install_from` against a local HTTP server. That needs no environment
variable and adds no public surface. `asset_url` is replaced by
`latest_asset_url(base, file)` and `tagged_asset_url(base, tag, file)`.

### Rejected alternatives

- **Pinned uv version or checksum table.** Ruled out by the constraints.
- **Verify the Sigstore attestation in process** with the `sigstore` crate. It
  pulls in a large dependency tree plus TUF trust-root handling for an opt-in
  convenience path. It's the strongest check, but too costly here.
- **Shell out to `gh attestation verify`.** `install.sh` does this for toolr's
  own archives, but it needs `gh` 2.56 or later, installed and authenticated,
  on the user's machine at an install prompt. Requiring it breaks the
  convenience path. Running it only when `gh` happens to be present silently
  downgrades to checksum-only, which `install.sh` deliberately refuses (see its
  `require` default).
- **Fetch the attestation API without verifying the signature.** That shows
  an attestation exists, not that it is valid.
- **Releases API `tag_name` plus asset `digest`.** One request and no race,
  but the unauthenticated limit is 60 requests an hour. With fail-closed, a
  NAT-shared CI fleet would get 403s and couldn't install uv.
- **The combined `sha256.sum`.** It gives the same assurance as the sidecar
  and needs a lookup by file name. The per-asset sidecar is simpler.

### What this does and doesn't protect against

It catches a truncated or corrupted download, a tampered CDN object, a
TLS-intercepting proxy that changes only the archive, and the `/latest` race.
It doesn't catch a compromised Astral release pipeline, because the archive
and sidecar come from the same release. Since releases are immutable, an
attacker would have to compromise the pipeline at publish time. Replacing
assets later isn't possible.

## User-visible behaviour

- A successful install looks the same, with no new output.
- A failed verification exits non-zero with the error above. Nothing is
  installed.
- Release note: "The uv auto-install now checks the downloaded archive against
  the SHA-256 checksum published with the same uv release, and refuses to
  install it if the checksum is missing or doesn't match."
- Docs: add a paragraph to `docs/installation/index.md` after the "Toolr
  downloads prebuilt uv binaries…" paragraph (around line 155), saying what is
  verified, what isn't, and that a failure means installing uv yourself.

## Testing

All tests are real: a local HTTP server serves a fake release. No internals are
mocked. The existing one-shot `serve_once` helper in `install.rs` becomes a
small router that answers several requests by path, with a status, headers and
a body for each.

- Happy path, `tar.gz` (unix) and `zip`: a 302 from
  `/releases/latest/download/<file>` to `/releases/download/v9.9.9/<file>`, the
  archive, and a correct sidecar. uv is installed, and the server saw the
  tagged sidecar and asset paths but not `/latest/download/<file>.sha256`.
- Uppercase hex and a `*`-prefixed file name in the sidecar are accepted.
- Digest mismatch gives `ChecksumMismatch`, and `dest` doesn't exist.
- Sidecar 404 gives `Unverified`, and `dest` doesn't exist.
- Malformed sidecars give `Unverified`: an empty file, 63 hex digits, a
  non-hex character, or a different file name.
- Tag resolution gives `Unverified` for a 200 instead of a 3xx, a 3xx with no
  `Location`, a `Location` with an unexpected path, and a tag of `..`.
- An unreachable host still gives `Http`. The existing test moves to
  `perform_install_from`.
- `user_message` for both new variants names the manual-install URL.

The `asset_url_points_at_latest_download` test is replaced by URL-builder
tests. Changed lines need 99% patch coverage.

## Open questions

1. **Bypass variable.** Should there be a `TOOLR_UV_SKIP_VERIFY=1`? The
   default proposal is no, because installing uv yourself is the escape
   hatch.
2. **Mirror or base-URL variable.** Should there be a documented
   `TOOLR_UV_DOWNLOAD_URL` for proxied or air-gapped mirrors? It would have to
   copy GitHub's `/releases/latest/download` redirect layout, and the checksum
   would then only be as trustworthy as the mirror. The default proposal is no,
   for now.
3. **Attestation later.** Should a follow-up add Sigstore verification once a
   light Rust verifier exists? This design leaves room for it as a second step
   after the checksum.
