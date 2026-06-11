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
NO_VERIFY=0
VERIFY_ATTESTATION="require"

print_help() {
  cat <<EOF
Install the toolr binary from a GitHub release.

Options:
  --version VERSION         Install a specific version (defaults to latest)
  --triple TRIPLE           Override host target triple (auto-detected)
  --prefix PREFIX           Install location (defaults to \$XDG_BIN_HOME or ~/.local/bin)
  --dry-run                 Print actions without making changes
  --no-verify               Skip SHA-256 verification (not recommended)
  --verify-attestation MODE Attestation verification mode: auto|require|skip
                            (default: require - needs the 'gh' CLI; pass
                            'skip' to bypass, accepting the supply-chain risk)
  -h, --help                Show this help
EOF
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
    --no-verify) NO_VERIFY=1; shift ;;
    --verify-attestation) VERIFY_ATTESTATION="$2"; shift 2 ;;
    --verify-attestation=*) VERIFY_ATTESTATION="${1#*=}"; shift ;;
    -h|--help) print_help; exit 0 ;;
    *) printf "install.sh: unknown argument: %s\n" "$1" >&2; exit 2 ;;
  esac
done

err() { printf "install.sh: %s\n" "$*" >&2; exit 1; }
info() { printf "install.sh: %s\n" "$*" >&2; }
warn() { printf "install.sh: WARNING: %s\n" "$*" >&2; }

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || err "missing required command: $1"
}

detect_triple() {
  uname_s="$(uname -s)"
  uname_m="$(uname -m)"
  case "$uname_s" in
    Linux)
      libc="gnu"
      if [ -f /etc/alpine-release ]; then libc="musl"; fi
      case "$uname_m" in
        x86_64|amd64) printf 'x86_64-unknown-linux-%s' "$libc" ;;
        aarch64|arm64) printf 'aarch64-unknown-linux-%s' "$libc" ;;
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

verify_sha256() {
  file="$1"; expected="$2"
  if [ "$NO_VERIFY" -eq 1 ]; then
    info "skipping checksum verification (--no-verify)"
    return 0
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$file" | awk '{print $1}')
  elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$file" | awk '{print $1}')
  else
    err "no sha256 tool available (install coreutils); rerun with --no-verify to skip"
  fi
  [ "$actual" = "$expected" ] || err "checksum mismatch: expected $expected got $actual"
}

# Verify the SLSA build-provenance attestation attached to a release
# archive via the GitHub CLI. Modes:
#   require - (default) fail hard if `gh` is missing or verification fails
#   auto    - verify if `gh` is on PATH; skip with a warning otherwise
#   skip    - never verify (e.g. when running offline)
# `require` is the default so a missing verifier can't silently downgrade
# the install to checksum-only (the `.sha256` ships from the same release,
# so it catches transport corruption, not a tampered release asset).
verify_attestation() {
  file="$1"
  case "$VERIFY_ATTESTATION" in
    skip)
      info "skipping attestation verification (--verify-attestation=skip)"
      return 0
      ;;
    auto|require) ;;
    *) err "invalid --verify-attestation value: $VERIFY_ATTESTATION (use auto|require|skip)" ;;
  esac
  if ! command -v gh >/dev/null 2>&1; then
    if [ "$VERIFY_ATTESTATION" = "require" ]; then
      printf '%s\n' \
        "install.sh: cannot verify the release's SLSA build provenance: the 'gh' CLI is not on PATH." \
        "  toolr archives are signed; verification needs GitHub CLI -> https://cli.github.com" \
        "  Choose one:" \
        "    * install 'gh' and re-run this installer (recommended), or" \
        "    * re-run with --verify-attestation=skip to install WITHOUT supply-chain verification." \
        >&2
      exit 1
    fi
    warn "skipping attestation verification ('gh' CLI not installed) -- the archive is NOT supply-chain verified"
    return 0
  fi
  info "verifying SLSA build provenance via 'gh attestation verify'"
  if ! gh attestation verify "$file" --repo "$REPO" >&2; then
    err "attestation verification failed for $file"
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
  if [ "$DRY_RUN" -eq 1 ]; then
    info "dry-run; exiting before download"
    return 0
  fi

  tmpdir="$(mktemp -d)"
  trap 'rm -rf "$tmpdir"' EXIT INT TERM

  fetch "$url" "${tmpdir}/${filename}"
  if [ "$NO_VERIFY" -eq 0 ]; then
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
