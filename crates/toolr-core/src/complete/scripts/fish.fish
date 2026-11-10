# toolr fish completion - delegates to `toolr __complete`.
#
# Install via `toolr self completion install fish`, or place this file
# at ~/.config/fish/completions/toolr.fish.
#
# Known limits: a word ending in a lone backslash matches nothing, and a value ending in a
# newline loses it.

function __toolr_complete
    # `commandline -opc` returns the tokens already on the command line,
    # excluding the in-progress word. `commandline -ct` returns the
    # in-progress word itself (may be empty).
    set -l tokens (commandline -opc)
    set -l raw (commandline -ct)
    # The engine matches the text the shell would pass, so take the typed quoting off;
    # fish escapes the replies again on insertion. `string unescape` never expands anything.
    # `string collect` keeps an unescaped newline inside the one word. Known limit: a value
    # ending in a newline loses it (`-N` would also keep the newline unescape prints).
    set -l current (string unescape -- "$raw" | string collect)
    # `string collect` fails on empty input, so check the unescape's own status.
    set -l ps $pipestatus
    if test "$ps[1]" != 0
        # Unescape fails on a lone trailing backslash; keep the word as typed.
        set current $raw
    else if not set -q current[1]
        # A lone `"` unescapes to nothing, and collect yields no element for it.
        set current ''
    end
    # Drop the leading `toolr` token.
    set -l args $tokens[2..-1]
    # Append the in-progress word as the trailing token. Do NOT use `--`
    # before it: fish's `set` builtin treats `--` as a literal value to
    # append, not an end-of-options marker, which would inject a spurious
    # `--` token into the call and trip clap's option-terminator logic.
    set -a args $current
    # Ask for the whole slot so fish matches it; an older toolr ignores this and prefix-filters.
    # Local, so it never leaks into the session.
    set -lx TOOLR_COMPLETE_MATCH shell
    set -l out (toolr __complete "$PWD" $args 2>/dev/null)
    set -l rc $status
    # `--opt=value`: the engine returns bare values, but fish matches them against the
    # whole word, so put `--opt=` back. Quoted, so an empty word is still one argument.
    # Values come from the unescaped `current`; paths from the raw word (see below), so the
    # two prefixes differ.
    set -l opt_prefix (string match -r -- '^--[^=]+=' "$current")
    # fish's path completers parse their argument as shell text, so they get the raw word.
    set -l raw_prefix (string match -r -- '^--[^=]+=' "$raw")
    set -l bare (string replace -r -- '^--[^=]+=' '' "$raw")
    # 10 = COMPLETE_PATHS, 11 = COMPLETE_DIRECTORIES in crates/toolr/src/dispatch/complete.rs (pinned by
    # crates/toolr/tests/complete_protocol_lockstep.rs; keep this line shape).
    switch $rc
        case 0
            for line in $out
                printf '%s\n' "$opt_prefix$line"
            end
        case 10
            # fish < 3.7 globs the whole `--opt=` word, so complete the bare path instead.
            for line in (__fish_complete_path "$bare")
                printf '%s\n' "$raw_prefix$line"
            end
        case 11
            for line in (__fish_complete_directories "$bare")
                printf '%s\n' "$raw_prefix$line"
            end
    end
end

complete -c toolr -f -a "(__toolr_complete)"
