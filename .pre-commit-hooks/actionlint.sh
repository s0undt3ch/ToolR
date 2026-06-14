#!/usr/bin/env bash
#
# Local actionlint pre-commit hook.
#
# Runs the `actionlint` binary from PATH (mise pins it in mise.toml) instead
# of the upstream `rhysd/actionlint` hook, whose default `actionlint` id uses
# `language: golang` and compiles the linter from source via the Go toolchain
# on every cold cache. mise already ships the exact pinned binary.
#
# Behaviour: if `actionlint` is on PATH, run it on the passed workflow files
# and exit with its status. If it is missing, error out — unless `--exit-zero`
# is passed, in which case warn and exit 0. Mirrors the `--exit-zero`
# convention of `pin-github-actions.py`.
set -euo pipefail

exit_zero=0
files=()
for arg in "$@"; do
	case "$arg" in
	--exit-zero) exit_zero=1 ;;
	*) files+=("$arg") ;;
	esac
done

if ! command -v actionlint >/dev/null 2>&1; then
	msg="actionlint not found on PATH. It is pinned in mise.toml — run 'mise install'."
	if [ "$exit_zero" -eq 1 ]; then
		echo "Warning: ${msg} Skipping actionlint (--exit-zero)." >&2
		exit 0
	fi
	echo "Error: ${msg}" >&2
	exit 1
fi

# Nothing to check (prek filtered everything out) — succeed quietly.
if [ "${#files[@]}" -eq 0 ]; then
	exit 0
fi

exec actionlint "${files[@]}"
