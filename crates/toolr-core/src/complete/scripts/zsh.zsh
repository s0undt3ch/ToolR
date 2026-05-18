#compdef toolr
# toolr zsh completion - delegates to `toolr __complete`.
#
# Install via `toolr self completion install zsh`, or place this file in
# a directory on your $fpath under the name `_toolr` and rerun
# `compinit`.

_toolr() {
    local -a candidates
    local cur
    cur="${words[CURRENT]}"

    # words[1] is `toolr`; pass the rest plus the in-progress word.
    local -a passthrough
    passthrough=("${(@)words[2,CURRENT]}")
    # When CURRENT points one past the last typed word, the in-progress
    # word is empty - make sure we still send an empty trailing token.
    if [[ ${#passthrough} -eq 0 ]]; then
        passthrough=("")
    fi

    candidates=("${(@f)$(toolr __complete "$PWD" "${passthrough[@]}" 2>/dev/null)}")

    if (( ${#candidates} > 0 )); then
        compadd -- "${candidates[@]}"
    fi
}

compdef _toolr toolr
