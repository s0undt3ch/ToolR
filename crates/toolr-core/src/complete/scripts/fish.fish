# toolr fish completion - delegates to `toolr __complete`.
#
# Install via `toolr self completion install fish`, or place this file
# at ~/.config/fish/completions/toolr.fish.

function __toolr_complete
    # `commandline -opc` returns the tokens already on the command line,
    # excluding the in-progress word. `commandline -ct` returns the
    # in-progress word itself (may be empty).
    set -l tokens (commandline -opc)
    set -l raw (commandline -ct)
    # The engine matches the text the shell would pass, so take the typed quoting off;
    # fish escapes the replies again on insertion. `string unescape` never expands anything.
    # `string collect` keeps an unescaped newline inside the one word. It also trims trailing
    # newlines, so a value ending in one loses it; `-N` would keep unescape's own newline too.
    set -l current (string unescape -- "$raw" | string collect)
    # `string collect` fails on empty input, so check the unescape: a lone `"` is an empty word.
    set -l ps $pipestatus
    if test "$ps[1]" != 0
        set current $raw
    else if not set -q current[1]
        set current ''
    end
    # Drop the leading `toolr` token.
    set -l args $tokens[2..-1]
    # Append the in-progress word as the trailing token. Do NOT use `--`
    # before it: fish's `set` builtin treats `--` as a literal value to
    # append, not an end-of-options marker, which would inject a spurious
    # `--` token into the call and trip clap's option-terminator logic.
    set -a args $current
    set -l out (toolr __complete "$PWD" $args 2>/dev/null)
    set -l rc $status
    # `--opt=value`: the engine returns bare values, but fish matches them against the
    # whole word, so put `--opt=` back. Quoted, so an empty word is still one argument.
    set -l opt_prefix (string match -r -- '^--[^=]+=' "$current")
    # fish's path completers parse their argument as shell text, so they get the raw word.
    set -l raw_prefix (string match -r -- '^--[^=]+=' "$raw")
    set -l bare (string replace -r -- '^--[^=]+=' '' "$raw")
    # 10 = COMPLETE_PATHS, 11 = COMPLETE_DIRECTORIES in crates/toolr/src/dispatch.rs.
    switch $rc
        case 0
            for line in $out
                printf '%s\n' "$opt_prefix$line"
            end
        # fish < 3.7 globs the whole `--opt=` word, so complete the bare path instead.
        case 10
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
