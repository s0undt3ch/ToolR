#!/usr/bin/env bash

set -e

uv run maturin develop --manifest-path crates/toolr-py/Cargo.toml
uv run pytest -s -ra -v "$@"
