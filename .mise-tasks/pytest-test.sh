#!/usr/bin/env bash

set -e

mise run develop-toolr-py
uv run pytest -s -ra -v "$@"
