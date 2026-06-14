#!/usr/bin/env bash
#
# Local gitleaks pre-commit hook.
#
# Runs the `gitleaks` binary from PATH (mise pins it in mise.toml) instead of
# the upstream `gitleaks/gitleaks` hook, whose default `gitleaks` id uses
# `language: golang` and compiles from source via the Go toolchain on every
# cold cache. mise already ships the exact pinned binary.
#
# Behaviour: if `gitleaks` is on PATH, scan the staged changes (the hook sets
# `pass_filenames: false`, so it takes no file arguments) and exit with its
# status. If it is missing, error out — unless `--exit-zero` is passed, in
# which case warn and exit 0. Mirrors the `--exit-zero` convention of
# `pin-github-actions.py`.
set -euo pipefail

exit_zero=0
for arg in "$@"; do
	case "$arg" in
	--exit-zero) exit_zero=1 ;;
	esac
done

if ! command -v gitleaks >/dev/null 2>&1; then
	msg="gitleaks not found on PATH. It is pinned in mise.toml — run 'mise install'."
	if [ "$exit_zero" -eq 1 ]; then
		echo "Warning: ${msg} Skipping gitleaks (--exit-zero)." >&2
		exit 0
	fi
	echo "Error: ${msg}" >&2
	exit 1
fi

exec gitleaks git --pre-commit --redact --staged --verbose
