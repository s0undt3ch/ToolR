#!/usr/bin/env bash
#MISE description="Build the toolr-py Python extension wheel (output → dist/)"
set -e

mkdir -p dist
uv run maturin build --manifest-path crates/toolr-py/Cargo.toml --out dist/ "$@"
