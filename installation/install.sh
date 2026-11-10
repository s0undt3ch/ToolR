#!/bin/sh
# toolr installer - fetches a release archive from GitHub and installs
# the `toolr` binary to $XDG_BIN_HOME (or ~/.local/bin).
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/s0undt3ch/ToolR/main/installation/install.sh | sh
#   curl -fsSL ...install.sh | sh -s -- --version 1.2.3 --triple x86_64-apple-darwin
set -eu

REPO="${TOOLR_REPO:-s0undt3ch/ToolR}"
VERSION=""
TRIPLE=""
PREFIX=""
DRY_RUN=0
VERIFY="full"
LEGACY_NO_CHECKSUM=0

print_help() {
  cat <<EOF
Install the toolr binary from a GitHub release.

Options:
  --version VERSION         Install a specific version (defaults to latest)
  --triple TRIPLE           Override host target triple (auto-detected)
  --prefix PREFIX           Install location (defaults to \$XDG_BIN_HOME or ~/.local/bin)
  --dry-run                 Print actions without making changes
  --verify MODE             How much to verify: full|auto|checksum|none
                            full      SHA-256 and the SLSA attestation (default; needs the 'gh' CLI)
                            auto      SHA-256, and the attestation when 'gh' is available
                            checksum  SHA-256 only
                            none      nothing (not recommended)
  -h, --help                Show this help
EOF
}

legacy_verify() {
  case "$1" in
    require) printf 'full' ;;
    skip) printf 'checksum' ;;
    *) printf '%s' "$1" ;;
  esac
}

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --version=*) VERSION="${1#*=}"; shift ;;
    --triple) TRIPLE="$2"; shift 2 ;;
    --triple=*) TRIPLE="${1#*=}"; shift ;;
    --prefix) PREFIX="$2"; shift 2 ;;
    --prefix=*) PREFIX="${1#*=}"; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    --verify) VERIFY="$2"; shift 2 ;;
    --verify=*) VERIFY="${1#*=}"; shift ;;
    # Old spellings, kept for one release. --no-verify skipped only the SHA-256 and left the
    # attestation as it was, so it stays a separate switch; the attestation modes map
    # require -> full, auto -> auto, skip -> checksum.
    --no-verify) LEGACY_NO_CHECKSUM=1; shift ;;
    --verify-attestation) VERIFY="$(legacy_verify "$2")"; shift 2 ;;
    --verify-attestation=*) VERIFY="$(legacy_verify "${1#*=}")"; shift ;;
    -h|--help) print_help; exit 0 ;;
    *) printf "install.sh: unknown argument: %s\n" "$1" >&2; exit 2 ;;
  esac
done

err() { printf "install.sh: %s\n" "$*" >&2; exit 1; }
info() { printf "install.sh: %s\n" "$*" >&2; }
warn() { printf "install.sh: WARNING: %s\n" "$*" >&2; }

case "$VERIFY" in
  full|auto|checksum|none) ;;
  *) err "invalid --verify value: $VERIFY (use full|auto|checksum|none)" ;;
esac

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || err "missing required command: $1"
}

detect_triple() {
  uname_s="$(uname -s)"
  uname_m="$(uname -m)"
  case "$uname_s" in
    Linux)
      # Static musl binary, ships for every Linux release — runs on glibc hosts too.
      case "$uname_m" in
        x86_64|amd64) printf 'x86_64-unknown-linux-musl' ;;
        aarch64|arm64) printf 'aarch64-unknown-linux-musl' ;;
        *) err "unsupported Linux architecture: $uname_m" ;;
      esac
      ;;
    Darwin)
      case "$uname_m" in
        x86_64) printf 'x86_64-apple-darwin' ;;
        arm64|aarch64) printf 'aarch64-apple-darwin' ;;
        *) err "unsupported macOS architecture: $uname_m" ;;
      esac
      ;;
    *) err "unsupported OS: $uname_s. Use install.ps1 on Windows." ;;
  esac
}

fetch() {
  url="$1"; dest="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$dest"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$dest" "$url"
  else
    err "neither curl nor wget is available"
  fi
}

# Print the lowercase hex SHA-256 of file $1. On failure, explain on stderr
# (naming the escape hatch $2) and return 1; the caller exits.
sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    digest=$(sha256sum < "$1" | awk '{print $1}')
  elif command -v shasum >/dev/null 2>&1; then
    digest=$(shasum -a 256 < "$1" | awk '{print $1}')
  elif command -v openssl >/dev/null 2>&1; then
    digest=$(openssl dgst -sha256 < "$1" | sed 's/^.*= *//')
  else
    printf 'install.sh: no sha256 tool available (install coreutils); rerun with %s to skip\n' "$2" >&2
    return 1
  fi
  # No pipefail in POSIX sh: a hasher that fails still leaves the pipeline at status 0.
  case "$digest" in
    ''|*[!0-9a-f]*) ;;
    *) if [ "${#digest}" -eq 64 ]; then printf '%s\n' "$digest"; return 0; fi ;;
  esac
  printf 'install.sh: could not compute the SHA-256 of %s; rerun with %s to skip\n' "$1" "$2" >&2
  return 1
}

verify_sha256() {
  file="$1"; expected="$2"
  if [ "$VERIFY" = "none" ] || [ "$LEGACY_NO_CHECKSUM" -eq 1 ]; then
    info "skipping checksum verification"
    return 0
  fi
  actual=$(sha256_of "$file" --verify=none) || exit 1
  [ "$actual" = "$expected" ] || err "checksum mismatch: expected $expected got $actual"
}

# Verify the SLSA build-provenance attestation attached to a release
# archive via the GitHub CLI. Modes (`--verify`):
#   full     - (default) fail hard if `gh` is missing or verification fails
#   auto     - verify if `gh` is on PATH; skip with a warning otherwise
#   checksum - never verify the attestation (e.g. when running offline)
#   none     - verify nothing, not even the SHA-256
# `full` is the default so a missing verifier can't silently downgrade
# the install to checksum-only (the `.sha256` ships from the same release,
# so it catches transport corruption, not a tampered release asset).
verify_attestation() {
  file="$1"
  case "$VERIFY" in
    checksum|none)
      info "skipping attestation verification (--verify=$VERIFY)"
      return 0
      ;;
    auto|full) ;;
    *) err "invalid --verify value: $VERIFY (use full|auto|checksum|none)" ;;
  esac
  if ! command -v gh >/dev/null 2>&1; then
    if [ "$VERIFY" = "full" ]; then
      printf '%s\n' \
        "install.sh: cannot verify the release's SLSA build provenance: the 'gh' CLI is not on PATH." \
        "  toolr archives are signed; verification needs GitHub CLI -> https://cli.github.com" \
        "  Choose one:" \
        "    * install 'gh' and re-run this installer (recommended), or" \
        "    * re-run with --verify=checksum to install WITHOUT supply-chain verification." \
        >&2
      exit 1
    fi
    warn "skipping attestation verification ('gh' CLI not installed) -- the archive is NOT supply-chain verified"
    return 0
  fi
  # gh before 2.56 can't load GitHub's current Sigstore trusted root, so
  # an old gh fails in `auto` too rather than skipping silently.
  gh_version=$(gh --version | sed -n 's/^gh version \([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p' | head -n 1)
  gh_major=${gh_version%%.*}
  gh_minor=${gh_version#*.}
  if [ -z "$gh_version" ] || [ "$gh_major" -lt 2 ] || { [ "$gh_major" -eq 2 ] && [ "$gh_minor" -lt 56 ]; }; then
    printf '%s\n' \
      "install.sh: gh >= 2.56 is required to verify attestations (found ${gh_version:-unknown})" \
      "  upgrade 'gh', or re-run with --verify=checksum to install WITHOUT supply-chain verification." \
      >&2
    exit 1
  fi
  # Same rules as action.yml's verify-attestation step: ci.yml shares the signer, so only
  # buildConfigURI tells a release from a CI build; one subject must carry this name and digest.
  SIGNER_WORKFLOW="https://github.com/${REPO}/.github/workflows/_build-binary-archive.yml@refs/heads/main"
  RELEASE_WORKFLOW="https://github.com/${REPO}/.github/workflows/release.yml@refs/heads/main"
  ARCHIVE_NAME="${file##*/}"
  ARCHIVE_SHA256=$(sha256_of "$file" --verify=checksum) || exit 1
  export ARCHIVE_NAME ARCHIVE_SHA256 RELEASE_WORKFLOW
  info "verifying SLSA build provenance via 'gh attestation verify'"
  if ! count=$(gh attestation verify "$file" \
    --repo "$REPO" \
    --cert-identity "$SIGNER_WORKFLOW" \
    --deny-self-hosted-runners \
    --format json \
    --jq '[.[].verificationResult
           | select(.signature.certificate.buildConfigURI == env.RELEASE_WORKFLOW
                    and any(.statement.subject[]; .name == env.ARCHIVE_NAME and .digest.sha256 == env.ARCHIVE_SHA256))]
          | length'); then
    err "attestation verification failed for $file"
  fi
  if ! [ "$count" -ge 1 ] 2>/dev/null; then
    printf '%s\n' \
      "install.sh: ${ARCHIVE_NAME} has a valid ${REPO} attestation, but none from the ${REPO} release workflow on main for this exact archive" \
      "  install a published release; --verify=checksum turns off this check entirely." \
      >&2
    exit 1
  fi
}

resolve_version() {
  need_cmd sed
  if [ -n "$VERSION" ]; then return; fi
  latest_url="https://github.com/${REPO}/releases/latest"
  if command -v curl >/dev/null 2>&1; then
    location=$(curl -sLI -o /dev/null -w '%{url_effective}\n' "$latest_url")
  elif command -v wget >/dev/null 2>&1; then
    location=$(wget --max-redirect=0 --server-response "$latest_url" 2>&1 | sed -n 's/^.*Location: //p' | tail -1)
    [ -n "$location" ] || location="$latest_url"
  else
    err "neither curl nor wget is available"
  fi
  tag=$(printf '%s' "$location" | sed 's|.*/tag/||' | sed 's|/$||')
  case "$tag" in
    v*) VERSION="${tag#v}" ;;
    *) err "could not parse latest version from $location" ;;
  esac
}

resolve_prefix() {
  if [ -n "$PREFIX" ]; then return; fi
  if [ -n "${XDG_BIN_HOME:-}" ]; then
    PREFIX="${XDG_BIN_HOME}"
  else
    PREFIX="$HOME/.local/bin"
  fi
}

main() {
  need_cmd uname
  [ -n "$TRIPLE" ] || TRIPLE="$(detect_triple)"
  resolve_version
  resolve_prefix

  filename="toolr-${VERSION}-${TRIPLE}.tar.gz"
  url="https://github.com/${REPO}/releases/download/v${VERSION}/${filename}"
  sha_url="${url}.sha256"
  info "version: ${VERSION}"
  info "triple:  ${TRIPLE}"
  info "prefix:  ${PREFIX}"
  info "url:     ${url}"
  info "verify:  ${VERIFY}$([ "$LEGACY_NO_CHECKSUM" -eq 1 ] && printf ' (no checksum)')"
  if [ "$DRY_RUN" -eq 1 ]; then
    info "dry-run; exiting before download"
    return 0
  fi

  tmpdir="$(mktemp -d)"
  trap 'rm -rf "$tmpdir"' EXIT INT TERM

  fetch "$url" "${tmpdir}/${filename}"
  if [ "$VERIFY" != "none" ] && [ "$LEGACY_NO_CHECKSUM" -eq 0 ]; then
    fetch "$sha_url" "${tmpdir}/${filename}.sha256"
    expected=$(awk '{print $1}' "${tmpdir}/${filename}.sha256")
    verify_sha256 "${tmpdir}/${filename}" "$expected"
  fi
  verify_attestation "${tmpdir}/${filename}"

  ( cd "$tmpdir" && tar -xzf "$filename" )
  extracted_dir="${tmpdir}/toolr-${VERSION}-${TRIPLE}"
  [ -d "$extracted_dir" ] || err "unexpected archive layout: $extracted_dir missing"

  mkdir -p "$PREFIX"
  install_target="${PREFIX}/toolr"
  cp "${extracted_dir}/toolr" "${install_target}.tmp"
  chmod +x "${install_target}.tmp"
  mv "${install_target}.tmp" "${install_target}"
  info "installed: ${install_target}"

  case ":${PATH}:" in
    *":${PREFIX}:"*) ;;
    *) info "note: ${PREFIX} is not on \$PATH; add it to your shell profile" ;;
  esac
}

main "$@"
