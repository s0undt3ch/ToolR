# shellcheck shell=bash
# toolr bash completion - delegates to `toolr __complete`.
#
# Install via `toolr self completion install bash`, or source this file
# directly. Re-source on every shell start; manifest contents are read
# at Tab time, not when this script is sourced.

_toolr_complete() {
    local cur prev words cword
    _init_completion || return

    # `prev` is populated by `_init_completion` per bash-completion
    # convention but unused below.
    : "$prev"

    # A prefix bash inserted earlier arrives backslash-escaped (`dry\ `),
    # while the engine matches raw values: `\x` stands for `x`.
    local raw_cur='' i c
    for ((i = 0; i < ${#cur}; i++)); do
        c=${cur:i:1}
        if [[ $c == \\ ]]; then
            i=$((i + 1))
            c=${cur:i:1}
        fi
        raw_cur+=$c
    done

    # Words after the cursor are dropped; `raw_cur` stands in for words[cword].
    local args=("${words[@]:1:cword-1}" "$raw_cur")

    # `|| rc=$?` keeps a non-zero exit from tripping the user's `set -e`.
    local raw rc=0
    raw=$(toolr __complete "$PWD" "${args[@]}" 2>/dev/null) || rc=$?
    # Exit 10 and 11 ask for file system paths and directories.
    case $rc in
        0) ;;
        10) _toolr_complete_paths -f "$raw_cur"; return 0 ;;
        11) _toolr_complete_paths -d "$raw_cur"; return 0 ;;
        *) return 0 ;;
    esac
    [[ -z "$raw" ]] && return 0

    # Each line is `value<TAB>description` - bash has no per-candidate
    # description support, so drop everything from the tab onward.
    # Candidates are repo-derived data: `compgen -W` would expand `$(...)`
    # inside a value, so compare literally and quote with `printf %q`.
    local value q bare_tilde
    # Bash < 5 `%q` leaves `~` bare, and bash tilde-expands it at a word start or after `=`.
    printf -v bare_tilde '%q' '~'
    while IFS=$'\t' read -r value _; do
        printf -v q '%q' "$value"
        [[ $bare_tilde == '~' ]] && q=${q//\~/\\~}
        # `cur` may already hold the quoted form from an earlier insertion.
        if [[ $value == "$raw_cur"* || $q == "$cur"* ]]; then
            COMPREPLY+=("$q")
        fi
    done <<< "$raw"
}

# File names are data too: `compgen` only lists them, and each is quoted with
# `%q` like a candidate value.
_toolr_complete_paths() {
    local mode=$1 raw_cur=$2 name rest q bare_tilde tilde dirs_list dirs=0
    printf -v bare_tilde '%q' '~'
    # `compgen -d` resolves `~user` itself, so no expansion of typed text is needed.
    [[ $mode == -f ]] && dirs_list=$'\n'$(compgen -d -- "$raw_cur")$'\n'
    while IFS= read -r name; do
        tilde=
        rest=$name
        # `compgen` echoes a typed leading `~`; it must stay bare to expand.
        if [[ $raw_cur == '~'* && $name == '~'* ]]; then
            tilde='~'
            rest=${name#\~}
        fi
        q=
        if [[ -n $rest ]]; then
            printf -v q '%q' "$rest"
            [[ $bare_tilde == '~' ]] && q=${q//\~/\\~}
        fi
        q=$tilde$q
        if [[ $mode == -d || $dirs_list == *$'\n'"$name"$'\n'* ]]; then
            q+=/
            dirs=$((dirs + 1))
        fi
        COMPREPLY+=("$q")
    done < <(compgen "$mode" -- "$raw_cur")
    # Bash 3.2 has no `compopt`, so a directory there gets a trailing space.
    if { [[ $mode == -d ]] || ((${#COMPREPLY[@]} == 1 && dirs == 1)); } &&
        type compopt &>/dev/null; then
        compopt -o nospace 2>/dev/null || :
    fi
}

complete -F _toolr_complete toolr
