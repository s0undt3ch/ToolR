#compdef toolr
# toolr zsh completion - delegates to `toolr __complete`.
#
# Install via `toolr self completion install zsh`, or place this file in
# a directory on your $fpath under the name `_toolr` and rerun
# `compinit`.

_toolr() {
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

    # Each line from `__complete` is `value<TAB>description`; split into
    # parallel arrays so `compadd -d` can show the description next to
    # its candidate.
    local -a lines values descriptions
    lines=("${(@f)$(toolr __complete "$PWD" "${passthrough[@]}" 2>/dev/null)}")
    # Saved first: the filter below would overwrite it.
    local rc=$?
    # `(@f)` turns empty output into one empty line; drop it, or `compadd` adds a blank match.
    lines=("${(@)lines:#}")
    # `--opt=value`: complete the part after the first `=`, keeping `--opt=` in the line.
    local attached=0
    [[ $cur == --?*=* ]] && attached=1
    # 10 = COMPLETE_PATHS, 11 = COMPLETE_DIRECTORIES in crates/toolr/src/dispatch.rs.
    case $rc in
        0) ;;
        10) (( attached )) && compset -P 1 '*='; _files; return ;;
        11) (( attached )) && compset -P 1 '*='; _files -/; return ;;
        *) return ;;
    esac
    # `compadd -d` uses the description array as the *displayed* text for
    # each match, not just an annotation - an empty description would show
    # as a blank row, so fall back to the value itself when none exists.
    local line value description
    for line in "${lines[@]}"; do
        value="${line%%$'\t'*}"
        description="${line#*$'\t'}"
        values+=("$value")
        descriptions+=("${description:-$value}")
    done

    if (( ${#values} == 0 )); then
        return
    fi
    if (( attached )); then
        compset -P 1 '*='
    fi

    # One-per-line descriptions only pay off for a small, scannable
    # candidate list; fall back to the compact grid for large ones (e.g.
    # long allowed-value sets) rather than a wall of text.
    if (( ${#values} <= 20 )); then
        compadd -d descriptions -a values
    else
        compadd -a values
    fi
}

compdef _toolr toolr
