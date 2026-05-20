#!/usr/bin/env bash
#MISE description="Build all packages (toolr, toolr-core, toolr-py)"
set -e

cargo build "$@"
uv run maturin develop
