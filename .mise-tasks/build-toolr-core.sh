#!/usr/bin/env bash
#MISE description="Build the toolr-core library"
set -e

cargo build -p toolr-core "$@"
