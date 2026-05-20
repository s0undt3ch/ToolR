#!/usr/bin/env bash
#MISE description="Remove all build artifacts (target/, dist/, .so files, __pycache__, .egg-info)"
set -e

cargo clean
rm -rf dist/

# Remove only the compiled extension modules that maturin drops into the
# project's own source tree. Scope to crates/ to avoid touching anything
# in .venv, site-packages, or mise-managed installs.
find ./crates -name "*.so" -exec rm -f {} +
find ./crates -name "*.pyd" -exec rm -f {} +

# Python bytecode cache (project source only, not .venv)
find . -path ./.venv -prune -o -type d -name "__pycache__" -exec rm -rf {} +

# egg-info directories left by editable installs / pytest
find . -path ./.venv -prune -o -type d -name "*.egg-info" -exec rm -rf {} +
