# shellcheck shell=bash
# toolr bash completion - delegates to `toolr __complete`.
#
# Install via `toolr self completion install bash`, or source this file
# directly. Re-source on every shell start; manifest contents are read
# at Tab time, not when this script is sourced.

# Sets the caller's `q` to `$1` quoted as one inert word, with no `eval`.
_toolr_quote() {
    local bare_tilde
    printf -v q '%q' "$1"
    # Bash < 5 `%q` leaves `~` bare, and bash tilde-expands it at a word start or after `=`.
    printf -v bare_tilde '%q' '~'
    if [[ $bare_tilde == '~' ]]; then
        q=${q//\~/\\~}
    fi
}

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
    # 10 = COMPLETE_PATHS, 11 = COMPLETE_DIRECTORIES in crates/toolr/src/dispatch.rs.
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
    local value q
    while IFS=$'\t' read -r value _; do
        _toolr_quote "$value"
        # `cur` may already hold the quoted form from an earlier insertion.
        if [[ $value == "$raw_cur"* || $q == "$cur"* ]]; then
            COMPREPLY+=("$q")
        fi
    done <<< "$raw"
}

# File names are data too: `compgen` only lists them, and each is quoted with
# `%q` like a candidate value.
_toolr_complete_paths() {
    local mode=$1 raw_cur=$2 name rest q tilde is_dir tilde_list='' dirs=0
    # Bash 3.2 errors on `${#COMPREPLY[@]}` under `set -u` while it is unset.
    COMPREPLY=()
    # `[[ -d ]]` can't test `~user/...` without expanding it, but `compgen -d` resolves it.
    if [[ $mode == -f && $raw_cur == '~'* ]]; then
        # A typed `~` rarely lists enough names for a string scan to matter.
        # `|| :` because `compgen` exits 1 on no match, which `set -e` would act on.
        tilde_list=$'\n'$(compgen -d -- "$raw_cur" || :)$'\n'
    fi
    while IFS= read -r name; do
        tilde=
        rest=$name
        # `compgen` echoes a typed leading `~`; it must stay bare to expand.
        if [[ $raw_cur == '~'* && $name == '~'* ]]; then
            tilde='~'
            rest=${name#\~}
        fi
        q=
        [[ -n $rest ]] && _toolr_quote "$rest"
        q=$tilde$q
        is_dir=0
        if [[ $mode == -d ]]; then
            is_dir=1
        elif [[ -z $tilde ]]; then
            # A test on the name `compgen` produced, so nothing is expanded.
            [[ -d $name ]] && is_dir=1
        else
            [[ $tilde_list == *$'\n'"$name"$'\n'* ]] && is_dir=1
        fi
        if ((is_dir)); then
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
