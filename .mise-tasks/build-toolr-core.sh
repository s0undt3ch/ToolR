#!/usr/bin/env bash
#MISE description="Build the toolr-core library (output → dist/)"
set -e

PROFILE=debug
for arg in "$@"; do
  [[ "$arg" == "--release" ]] && PROFILE=release
done

cargo build -p toolr-core "$@"
mkdir -p dist
cp "target/$PROFILE/libtoolr_core."* dist/
