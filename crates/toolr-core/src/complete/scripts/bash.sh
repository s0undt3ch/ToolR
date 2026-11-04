# shellcheck shell=bash
# toolr bash completion - delegates to `toolr __complete`. Needs bash 4.4 or newer.
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
    # Bash splits COMP_WORDS at COMP_WORDBREAKS (`a:b` is `a`, `:`, `b`): glue the
    # pieces back together where COMP_LINE has no whitespace between them.
    local line=${COMP_LINE:0:COMP_POINT} words=() n=0 i w ws
    for ((i = 0; i <= COMP_CWORD; i++)); do
        ws=${line%%[![:space:]]*}
        line=${line#"$ws"}
        if ((i == COMP_CWORD)); then
            # A cursor inside a word completes only what precedes it.
            w=$line
        else
            w=${COMP_WORDS[i]}
            line=${line#"$w"}
        fi
        if ((i > 0)) && [[ -z $ws ]]; then
            words[n - 1]+=$w
        else
            words[n]=$w
            n=$((n + 1))
        fi
    done
    local cur=${words[n - 1]}

    # A prefix bash inserted earlier arrives backslash-escaped (`dry\ `),
    # while the engine matches raw values: `\x` stands for `x`. Readline replaces
    # only the text after the last unescaped wordbreak, so keep the raw text up to it.
    # `$` and `@` are bash's readline special prefixes: the replaced text starts at them,
    # so they stay in the reply. Readline doesn't break inside an open quote, so such a
    # word is left untrimmed.
    local breaks=${COMP_WORDBREAKS-}
    breaks=${breaks//[$' \t\n\'"']/}
    local raw_cur='' raw_prefix='' c
    for ((i = 0; i < ${#cur}; i++)); do
        c=${cur:i:1}
        if [[ $c == \\ ]]; then
            i=$((i + 1))
            raw_cur+=${cur:i:1}
            continue
        fi
        raw_cur+=$c
        if [[ -n $breaks && $breaks == *"$c"* ]]; then
            raw_prefix=$raw_cur
            if [[ $c == [\$@] ]]; then
                raw_prefix=${raw_cur%?}
            fi
        fi
    done

    # Words after the cursor are dropped; `raw_cur` stands in for the cursor's word.
    local args=()
    for ((i = 1; i < n - 1; i++)); do
        args+=("${words[i]}")
    done
    args+=("$raw_cur")

    # `|| rc=$?` keeps a non-zero exit from tripping the user's `set -e`.
    local raw rc=0
    raw=$(toolr __complete "$PWD" "${args[@]}" 2>/dev/null) || rc=$?
    # 10 = COMPLETE_PATHS, 11 = COMPLETE_DIRECTORIES in crates/toolr/src/dispatch.rs.
    case $rc in
        0) ;;
        10) _toolr_complete_paths -f "$raw_cur" "$raw_prefix"; return 0 ;;
        11) _toolr_complete_paths -d "$raw_cur" "$raw_prefix"; return 0 ;;
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
            [[ -n $raw_prefix ]] && _toolr_quote "${value#"$raw_prefix"}"
            COMPREPLY+=("$q")
        fi
    done <<< "$raw"
}

# File names are data too: `compgen` only lists them, and each is quoted with
# `%q` like a candidate value. `$3` is the typed text readline keeps.
_toolr_complete_paths() {
    local mode=$1 raw_cur=$2 raw_prefix=${3-} name rest q tilde is_dir dirs=0
    local -A tilde_dirs=()
    # `${#COMPREPLY[@]}` errors under `set -u` while the array is unset.
    COMPREPLY=()
    # `[[ -d ]]` can't test `~user/...` without expanding it, but `compgen -d` resolves it.
    if [[ $mode == -f && $raw_cur == '~'* ]]; then
        # `|| :` because `compgen` exits 1 on no match, which `set -e` would act on.
        while IFS= read -r name; do
            tilde_dirs[$name]=1
        done < <(compgen -d -- "$raw_cur" || :)
    fi
    while IFS= read -r name; do
        tilde=
        rest=$name
        if [[ -n $raw_prefix ]]; then
            rest=${name#"$raw_prefix"}
        # `compgen` echoes a typed leading `~`; it must stay bare to expand.
        elif [[ $raw_cur == '~'* && $name == '~'* ]]; then
            tilde='~'
            rest=${name#\~}
        fi
        q=
        [[ -n $rest ]] && _toolr_quote "$rest"
        q=$tilde$q
        is_dir=0
        if [[ $mode == -d ]]; then
            is_dir=1
        elif [[ $raw_cur == '~'* ]]; then
            [[ -n ${tilde_dirs[$name]-} ]] && is_dir=1
        else
            # A test on the name `compgen` produced, so nothing is expanded.
            [[ -d $name ]] && is_dir=1
        fi
        if ((is_dir)); then
            q+=/
            dirs=$((dirs + 1))
        fi
        COMPREPLY+=("$q")
    done < <(compgen "$mode" -- "$raw_cur")
    if [[ $mode == -d ]] || ((${#COMPREPLY[@]} == 1 && dirs == 1)); then
        compopt -o nospace 2>/dev/null || :
    fi
}

# Bash 3.2-safe syntax, so an older bash sources this without an error.
# Only an interactive shell warns: a bashrc also runs under `bash -c` and scripts.
if ((BASH_VERSINFO[0] * 100 + BASH_VERSINFO[1] < 404)); then
    # An `if`, not `&&`: a false test would make `source` return 1 under the user's `set -e`.
    if [[ $- == *i* ]]; then
        printf 'toolr: tab completion needs bash >= 4.4 (this is %s); upgrade bash to enable it.\n' \
            "$BASH_VERSION" >&2
    fi
else
    complete -F _toolr_complete toolr
fi
