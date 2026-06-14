#!/usr/bin/env bash
#
# Local shellcheck pre-commit hook.
#
# Runs the `shellcheck` binary from PATH (mise pins it in mise.toml) instead
# of pulling the upstream Docker image (koalaman/shellcheck-precommit uses
# `language: docker_image`, which needs a Docker daemon and a registry pull —
# unavailable on hardened CI runners and wasteful when mise already provides
# the exact pinned binary).
#
# Behaviour: if `shellcheck` is on PATH, run it on the passed files and exit
# with its status (acts exactly like the upstream hook). If it is missing,
# error out — unless `--exit-zero` is passed, in which case warn and exit 0
# (a non-blocking, tool-optional advisory run). Mirrors the `--exit-zero`
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

if ! command -v shellcheck >/dev/null 2>&1; then
	msg="shellcheck not found on PATH. It is pinned in mise.toml — run 'mise install'."
	if [ "$exit_zero" -eq 1 ]; then
		echo "Warning: ${msg} Skipping shellcheck (--exit-zero)." >&2
		exit 0
	fi
	echo "Error: ${msg}" >&2
	exit 1
fi

# Nothing to check (prek filtered everything out) — succeed quietly.
if [ "${#files[@]}" -eq 0 ]; then
	exit 0
fi

exec shellcheck "${files[@]}"
