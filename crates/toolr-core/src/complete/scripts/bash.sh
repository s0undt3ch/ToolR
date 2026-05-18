# shellcheck shell=bash
# toolr bash completion - delegates to `toolr __complete`.
#
# Install via `toolr self completion install bash`, or source this file
# directly. Re-source on every shell start; manifest contents are read
# at Tab time, not when this script is sourced.

_toolr_complete() {
    local cur prev words cword
    _init_completion || return

    # `cword` and `prev` are populated by `_init_completion` per
    # bash-completion convention even though only `cur` and `words` are
    # used below.
    : "$cword" "$prev"

    # Pass everything after `toolr` (words[0]) to the binary. `cur` is
    # already the trailing in-progress word; include it as the final
    # element so the engine treats it as the prefix.
    local args=("${words[@]:1}")

    local IFS=$'\n'
    local candidates
    candidates=$(toolr __complete "$PWD" "${args[@]}" 2>/dev/null) || return 0

    # shellcheck disable=SC2207
    COMPREPLY=($(compgen -W "$candidates" -- "$cur"))
}

complete -F _toolr_complete toolr
