#!/usr/bin/env bash
#MISE description="Build all packages (toolr, toolr-core, toolr-py) (output → dist/)"
set -e

PROFILE=debug
for arg in "$@"; do
  [[ "$arg" == "--release" ]] && PROFILE=release
done

cargo build "$@"
mkdir -p dist
cp "target/$PROFILE/toolr" dist/
cp "target/$PROFILE/libtoolr_core."* dist/
uv run maturin build --manifest-path crates/toolr-py/Cargo.toml --out dist/
