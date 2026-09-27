#!/usr/bin/env bash
# Usage: packslip-check.sh <archive-dir>
# Signs a throwaway bundle from .github/packslip.toml + the archives, then
# verifies it, catching manifest/archive drift before release day.
set -euo pipefail

fail() {
  printf 'packslip-check: %s failed: %s\n' "$1" "$2" >&2
  exit 1
}

# Resolves $1 to an absolute path before we cd to the repo root, so a
# relative archive-dir/PACKSLIP/PACKSLIP_MANIFEST stays anchored to the
# caller's cwd instead of silently being reinterpreted against the repo.
abspath() {
  local path=$1 dir base
  dir=$(cd "$(dirname "$path")" 2>/dev/null && pwd) || return 1
  base=$(basename "$path")
  printf '%s/%s\n' "$dir" "$base"
}

if [ "$#" -ne 1 ]; then
  fail "usage" "expected exactly one argument: <archive-dir>"
fi
archive_dir=$(cd "$1" 2>/dev/null && pwd) || fail "archives" "$1 is not a directory"

PACKSLIP=${PACKSLIP:-packslip}
case "$PACKSLIP" in
  */*) PACKSLIP=$(abspath "$PACKSLIP") || fail "packslip" "$PACKSLIP not found" ;;
esac

if [ -n "${PACKSLIP_MANIFEST+x}" ]; then
  manifest=$(abspath "$PACKSLIP_MANIFEST") || fail "manifest" "$PACKSLIP_MANIFEST not found"
else
  manifest=.github/packslip.toml
fi

repo_root=$(git rev-parse --show-toplevel) || fail "repo root" "not inside a git repository"
cd "$repo_root"

[ -f "$manifest" ] || fail "manifest" "$manifest not found"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

if ! keygen_out=$("$PACKSLIP" keygen -o "$tmp/k" 2>&1); then
  fail "packslip keygen" "$keygen_out"
fi

archives=()
while IFS= read -r -d '' f; do
  archives+=("$f")
done < <(find "$archive_dir" -maxdepth 1 -type f \( -name '*.tar.gz' -o -name '*.zip' \) -print0 | sort -z)

if [ "${#archives[@]}" -eq 0 ]; then
  fail "archives" "no *.tar.gz or *.zip files found in $archive_dir"
fi

commit=$(git rev-parse HEAD)
if ! create_out=$("$PACKSLIP" create --manifest "$manifest" --key "$tmp/k" --no-log \
  --out "$tmp/out" --project github.com/s0undt3ch/ToolR --version 0.0.0-ci \
  --commit "$commit" --source-repo https://github.com/s0undt3ch/ToolR \
  "${archives[@]}" 2>&1); then
  fail "packslip create" "$create_out"
fi

bundle="$tmp/out/packslip.sigstore.json"
[ -f "$bundle" ] || fail "packslip create" "expected bundle at $bundle, none produced"

verify_args=()
for f in "${archives[@]}"; do
  verify_args+=("--artifact" "$f")
done

if ! verify_out=$("$PACKSLIP" verify "$bundle" --pubkey "$tmp/k.pub" --allow-unlogged \
  "${verify_args[@]}" 2>&1); then
  fail "packslip verify" "$verify_out"
fi

if ! statement=$("$PACKSLIP" show "$bundle" 2>&1); then
  fail "packslip show" "$statement"
fi

if ! bin_violations=$(jq -r '
  .predicate.artifacts[] |
  . as $a |
  ($a.bin // []) as $bin |
  ($bin | length) as $n |
  (if $n == 1 then ($bin[0] | if type == "object" then .path else . end) else null end) as $p |
  if $n != 1 then
    "\($a.name): expected exactly one bin entry, got \($n)"
  elif ($p | test("/toolr$|/toolr\\.exe$") | not) then
    "\($a.name): bin entry \($p) does not end in /toolr or /toolr.exe"
  else empty end
' <<<"$statement" 2>&1); then
  fail "artifact bin entries" "$bin_violations"
fi
[ -z "$bin_violations" ] || fail "artifact bin entries" "$bin_violations"

if ! musl_violations=$(jq -r '
  .predicate.artifacts[] |
  select(.os == "linux") |
  select(.libc != "musl") |
  "\(.name): os=linux but libc=\(.libc // "null") (expected musl)"
' <<<"$statement" 2>&1); then
  fail "linux libc" "$musl_violations"
fi
[ -z "$musl_violations" ] || fail "linux libc" "$musl_violations"

[ -d skills ] || fail "skill resources" "skills/ directory not found"

if ! manifest_skills=$(jq -r '[.predicate.resources[] | select(.kind == "skill") | .name] | sort | .[]' \
  <<<"$statement" 2>&1); then
  fail "skill resources" "$manifest_skills"
fi

dir_skills=$(
  for d in skills/*/; do
    d=${d%/}
    if [ -f "$d/SKILL.md" ]; then
      basename "$d"
    fi
  done | sort
)

if [ "$manifest_skills" != "$dir_skills" ]; then
  fail "skill resources" "manifest skill resources ($(echo "$manifest_skills" | tr '\n' ' ')) do not match skills/*/SKILL.md directories ($(echo "$dir_skills" | tr '\n' ' '))"
fi

if ! repo_violations=$(jq -r '
  .predicate.resources[] |
  select(.kind == "skill") |
  select(.repo != "skills/\(.name)") |
  "\(.name): repo is \(.repo // "null"), expected skills/\(.name)"
' <<<"$statement" 2>&1); then
  fail "skill repo paths" "$repo_violations"
fi
[ -z "$repo_violations" ] || fail "skill repo paths" "$repo_violations"

while IFS= read -r name; do
  [ -n "$name" ] || continue
  skill_file="skills/$name/SKILL.md"
  frontmatter_name=$(awk '
    NR == 1 {
      if ($0 ~ /^---[[:space:]]*\r?$/) { in_fm = 1; next }
      exit
    }
    in_fm && /^---[[:space:]]*\r?$/ { exit }
    in_fm && /^name:[[:space:]]*/ {
      line = $0
      sub(/^name:[[:space:]]*/, "", line)
      sub(/\r$/, "", line)
      sub(/[[:space:]]+$/, "", line)
      print line
      exit
    }
  ' "$skill_file")

  len=${#frontmatter_name}
  if [ "$len" -ge 2 ]; then
    first_char=${frontmatter_name:0:1}
    last_char=${frontmatter_name:$((len - 1)):1}
    if { [ "$first_char" = '"' ] && [ "$last_char" = '"' ]; } ||
      { [ "$first_char" = "'" ] && [ "$last_char" = "'" ]; }; then
      frontmatter_name=${frontmatter_name:1:$((len - 2))}
    fi
  fi

  if [ "$frontmatter_name" != "$name" ]; then
    fail "skill frontmatter" "$skill_file has name: '$frontmatter_name', expected '$name'"
  fi
done <<<"$manifest_skills"

if ! completion_err=$(jq -e '
  .predicate.resources | any(.kind == "completion" and .shells == ["bash", "zsh", "fish"])
' <<<"$statement" 2>&1 1>/dev/null); then
  if [ -n "$completion_err" ]; then
    fail "completion resource" "$completion_err"
  fi
  fail "completion resource" "no completion resource with shells == [\"bash\", \"zsh\", \"fish\"] found"
fi

echo "packslip-check: ok"
