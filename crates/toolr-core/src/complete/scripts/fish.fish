# toolr fish completion - delegates to `toolr __complete`.
#
# Install via `toolr self completion install fish`, or place this file
# at ~/.config/fish/completions/toolr.fish.

function __toolr_complete
    # `commandline -opc` returns the tokens already on the command line,
    # excluding the in-progress word. `commandline -ct` returns the
    # in-progress word itself (may be empty).
    set -l tokens (commandline -opc)
    set -l current (commandline -ct)
    # Drop the leading `toolr` token.
    set -l args $tokens[2..-1]
    # Append the in-progress word as the trailing token. Do NOT use `--`
    # before it: fish's `set` builtin treats `--` as a literal value to
    # append, not an end-of-options marker, which would inject a spurious
    # `--` token into the call and trip clap's option-terminator logic.
    set -a args $current
    toolr __complete "$PWD" $args 2>/dev/null
end

complete -c toolr -f -a "(__toolr_complete)"
