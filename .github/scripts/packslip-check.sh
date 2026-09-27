#!/usr/bin/env bash
# Usage: packslip-check.sh <archive-dir>
# Signs a throwaway bundle from .github/packslip.toml + the archives, then
# verifies it, catching manifest/archive drift before release day.
set -euo pipefail

fail() {
  printf 'packslip-check: %s failed: %s\n' "$1" "$2" >&2
  exit 1
}

if [ "$#" -ne 1 ]; then
  fail "usage" "expected exactly one argument: <archive-dir>"
fi
archive_dir=$1

PACKSLIP=${PACKSLIP:-packslip}
manifest=${PACKSLIP_MANIFEST:-.github/packslip.toml}

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

bin_violations=$(jq -r '
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
' <<<"$statement")
[ -z "$bin_violations" ] || fail "artifact bin entries" "$bin_violations"

musl_violations=$(jq -r '
  .predicate.artifacts[] |
  select(.os == "linux") |
  select(.libc != "musl") |
  "\(.name): os=linux but libc=\(.libc // "null") (expected musl)"
' <<<"$statement")
[ -z "$musl_violations" ] || fail "linux libc" "$musl_violations"

manifest_skills=$(jq -r '[.predicate.resources[] | select(.kind == "skill") | .name] | sort | .[]' <<<"$statement")
dir_skills=$(
  for d in skills/*/; do
    d=${d%/}
    [ -f "$d/SKILL.md" ] && basename "$d"
  done | sort
)
if [ "$manifest_skills" != "$dir_skills" ]; then
  fail "skill resources" "manifest skill resources ($(echo "$manifest_skills" | tr '\n' ' ')) do not match skills/*/SKILL.md directories ($(echo "$dir_skills" | tr '\n' ' '))"
fi

while IFS= read -r name; do
  [ -n "$name" ] || continue
  skill_file="skills/$name/SKILL.md"
  frontmatter_name=$(awk '
    /^---[[:space:]]*$/ { if (in_fm) { exit } in_fm = 1; next }
    in_fm && /^name:[[:space:]]*/ { sub(/^name:[[:space:]]*/, ""); print; exit }
  ' "$skill_file")
  if [ "$frontmatter_name" != "$name" ]; then
    fail "skill frontmatter" "$skill_file has name: '$frontmatter_name', expected '$name'"
  fi
done <<<"$manifest_skills"

if ! jq -e '.predicate.resources | any(.kind == "completion" and .shells == ["bash", "zsh", "fish"])' \
  <<<"$statement" >/dev/null; then
  fail "completion resource" "no completion resource with shells == [\"bash\", \"zsh\", \"fish\"] found"
fi

echo "packslip-check: ok"
