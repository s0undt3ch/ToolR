#!/usr/bin/env bash

set -e

uv run maturin develop
uv run pytest -s -ra -v "$@"
