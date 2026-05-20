#!/usr/bin/env bash
#MISE description="Remove all build artifacts (target/, dist/)"
set -e

cargo clean
rm -rf dist/
