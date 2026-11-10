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
    local -a lines candidates
    lines=("${(@f)$(TOOLR_COMPLETE_MATCH=shell toolr __complete "$PWD" "${passthrough[@]}" 2>/dev/null)}")
    # Saved first: the filter below would overwrite it.
    local rc=$?
    # `(@f)` turns empty output into one empty line; drop it, or `compadd` adds a blank match.
    lines=("${(@)lines:#}")
    # `--opt=value`: complete the part after the first `=`, keeping `--opt=` in the line.
    local attached=0
    [[ $cur == --?*=* ]] && attached=1
    # 10 = COMPLETE_PATHS, 11 = COMPLETE_DIRECTORIES in crates/toolr/src/dispatch.rs (pinned by
    # crates/toolr/tests/complete_protocol_lockstep.rs; keep this line shape).
    # `_files` reads the raw word: zsh's `_files` dequotes `PREFIX` itself.
    case $rc in
        0) ;;
        10) (( attached )) && compset -P 1 '*='; _files; return ;;
        11) (( attached )) && compset -P 1 '*='; _files -/; return ;;
        *) return ;;
    esac
    # One expansion, not a loop: unfiltered, a slot can hold thousands of lines. The only loop
    # below visits at most `max_described` of them.
    candidates=("${(@)lines%%$'\t'*}")

    if (( ${#candidates} == 0 )); then
        return
    fi
    if (( attached )); then
        compset -P 1 '*='
    fi

    # A bare `compadd` ignores `matcher-list`; `_description` carries it in `expl`. The tag is
    # neutral because the reply doesn't say if the slot holds commands, flags or values.
    local -a expl
    _description toolr expl toolr

    # One-per-line descriptions only pay off for a small list; use the compact grid for large
    # ones. `-O` counts what the typed word matches without adding anything. No `-Q`: zsh
    # re-quotes each match to fit the quote open at the cursor, so values stay inert text.
    local -i max_described=20
    local -a matched
    compadd "${expl[@]}" -O matched -a candidates
    if (( ${#matched} <= max_described )); then
        # `-d` replaces what the listing shows, so each entry carries its own name, padded to the
        # widest listed value (`m` counts display width) so the descriptions line up and hidden
        # candidates don't pad the visible rows; `-l` puts one entry on each row. Only the
        # `max_described` or fewer matches are visited: one pass over every candidate of a large
        # slot would be slow.
        # The test stub in crates/toolr/tests/complete_shells.rs reads `shown_values` and
        # `display` by dynamic scope; keep the names in step.
        local -a shown_values display
        local -i width=0 idx
        local v line desc
        shown_values=("${matched[@]}")
        for v in "${shown_values[@]}"; do
            (( ${(m)#v} > width )) && width=${(m)#v}
        done
        for v in "${shown_values[@]}"; do
            # `matched` is a subset the matcher may have reordered, so each match finds its own
            # line by value. `(e)` makes `v` literal: a value such as `a*c` is not a pattern.
            idx=${candidates[(ie)$v]}
            line=${lines[idx]}
            # Drop an empty trailing description, then everything up to the first tab; a line
            # with no tab keeps the whole line, so test for text after a tab to see if there
            # is a description.
            desc=${${line%$'\t'}#*$'\t'}
            if [[ $line == *$'\t'?* ]]; then
                display+=("${(mr:width:)v}  -- $desc")
            else
                display+=("$v")
            fi
        done
        compadd "${expl[@]}" -l -d display -a shown_values
    else
        compadd "${expl[@]}" -a candidates
    fi
}

# Autoloaded: `compinit` already registered `toolr` via `#compdef`, and this file is the body
# of `_toolr`, so run the function it just defined. Sourced: register it.
if [[ ${zsh_eval_context[-1]-} == loadautofunc ]]; then
    _toolr "$@"
else
    compdef _toolr toolr
fi
