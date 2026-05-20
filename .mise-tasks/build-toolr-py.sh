#!/usr/bin/env bash
#MISE description="Build the toolr-py Python extension (maturin develop)"
set -e

uv run maturin develop --manifest-path crates/toolr-py/Cargo.toml "$@"
