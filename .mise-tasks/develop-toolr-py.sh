#!/usr/bin/env bash
#MISE description="Install toolr-py as editable in .venv (maturin develop)"
set -e

uv run maturin develop --manifest-path crates/toolr-py/Cargo.toml "$@"
