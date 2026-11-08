#compdef toolr
# toolr zsh completion - delegates to `toolr __complete`.
#
# Install via `toolr self completion install zsh`, or place this file in
# a directory on your $fpath under the name `_toolr` and rerun
# `compinit`.
#
# Known limit: escapes inside `$'…'` (`$'dry\x20w`) don't complete, because zsh's matcher
# compares candidates against the raw `PREFIX`.

_toolr() {
    # Defensive: inside backticks zsh completes the command itself and doesn't call us.
    [[ ${compstate[quote]-} == '`' ]] && return
    # `$words` holds each word as typed; the engine matches the text the shell would pass.
    # `(Q)` only removes quotes, it never expands or runs anything.
    local raw cur
    raw="${words[CURRENT]}"
    cur="${(Q)raw}"
    # `(Q)` leaves a word with an open quote untouched, so close it first. `compstate[quote]`
    # is the quote at the cursor (`'`, `"` or `$'`, each closed by its last character), but
    # zsh sets it only when the quote opens the word, so a mid-word quote tries `'` then `"`.
    if [[ $cur == "$raw" && $raw == *[\'\"]* ]]; then
        local q
        # Unquoted, an empty `compstate[quote]` vanishes instead of trying an empty `q`; `: -1`
        # takes the closing character of `$'`.
        for q in ${compstate[quote]: -1} \' \"; do
            if [[ ${(Q):-$raw$q} != "$raw$q" ]]; then
                cur="${(Q):-$raw$q}"
                break
            fi
        done
    fi

    # words[1] is `toolr`; pass the rest plus the in-progress word. `@` keeps empty and quoted
    # words as one each, and `(Q)` lets the engine see `'--mode'` as the option it is.
    local -a passthrough
    passthrough=("${(@Q)words[2,CURRENT-1]}" "$cur")

    # Each line from `__complete` is `value<TAB>description`. `TOOLR_COMPLETE_MATCH=shell` asks
    # for every candidate in the slot, so `compadd` applies the user's `matcher-list`; a toolr
    # that predates it ignores the variable and filters by prefix.
    local -a lines values descriptions
    lines=("${(@f)$(TOOLR_COMPLETE_MATCH=shell toolr __complete "$PWD" "${passthrough[@]}" 2>/dev/null)}")
    # Saved first: the filter below would overwrite it.
    local rc=$?
    # `(@f)` turns empty output into one empty line; drop it, or `compadd` adds a blank match.
    lines=("${(@)lines:#}")
    # `--opt=value`: complete the part after the first `=`, keeping `--opt=` in the line.
    local attached=0
    [[ $cur == --?*=* ]] && attached=1
    # 10 = COMPLETE_PATHS, 11 = COMPLETE_DIRECTORIES in crates/toolr/src/dispatch.rs.
    # `_files` reads the raw word: zsh's `_files` dequotes `PREFIX` itself.
    case $rc in
        0) ;;
        10) (( attached )) && compset -P 1 '*='; _files; return ;;
        11) (( attached )) && compset -P 1 '*='; _files -/; return ;;
        *) return ;;
    esac
    # One expansion, not a loop: unfiltered, a large choice set can be thousands of lines.
    values=("${(@)lines%%$'\t'*}")

    if (( ${#values} == 0 )); then
        return
    fi
    if (( attached )); then
        compset -P 1 '*='
    fi

    # A bare `compadd` ignores the `matcher-list` style; `_description` puts its matcher (and
    # the user's other styles for the tag) into `expl`. The reply doesn't say whether the slot
    # holds commands, flags or values, so the tag and its `%d` header are neutral rather than
    # a `values` that would also restyle command and flag names.
    local -a expl
    _description toolr expl toolr

    # One-per-line descriptions only pay off for a small, scannable list of matches; fall back
    # to the compact grid for large ones rather than a wall of text. The engine returns the
    # whole slot, so `-O` counts what the typed word matches without adding anything. No `-Q`:
    # zsh re-quotes each match to fit the quote open at the cursor, so values stay inert text.
    local -a matched
    compadd "${expl[@]}" -O matched -a values
    if (( ${#matched} <= 20 )); then
        # `compadd -d` uses the description array as the *displayed* text for each match, not
        # just an annotation, so a line with no description (or an empty one) shows the value.
        descriptions=("${(@)${(@)lines%$'\t'}#*$'\t'}")
        compadd "${expl[@]}" -d descriptions -a values
    else
        compadd "${expl[@]}" -a values
    fi
}

# Autoloaded: `compinit` already registered `toolr` via `#compdef`, and this file is the body
# of `_toolr`, so run the function it just defined. Sourced: register it.
if [[ ${zsh_eval_context[-1]-} == loadautofunc ]]; then
    _toolr "$@"
else
    compdef _toolr toolr
fi
