#!/usr/bin/env bash
#MISE description="Remove all build artifacts (target/, dist/, .so files, __pycache__, .egg-info)"
set -e

cargo clean
rm -rf dist/

# maturin develop drops compiled extension modules into the source tree
find . -path ./.venv -prune -o \( -name "*.so" -o -name "*.pyd" \) -exec rm -f {} +

# Python bytecode cache
find . -path ./.venv -prune -o -type d -name "__pycache__" -exec rm -rf {} +

# egg-info directories left by editable installs / pytest
find . -path ./.venv -prune -o -type d -name "*.egg-info" -exec rm -rf {} +
