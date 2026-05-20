#!/usr/bin/env bash
#MISE description="Build the toolr CLI binary"
set -e

cargo build -p toolr "$@"
