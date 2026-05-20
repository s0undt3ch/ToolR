#!/usr/bin/env bash
#MISE description="Build the toolr CLI binary (output → dist/)"
set -e

PROFILE=debug
for arg in "$@"; do
  [[ "$arg" == "--release" ]] && PROFILE=release
done

cargo build -p toolr "$@"
mkdir -p dist
cp "target/$PROFILE/toolr" dist/
