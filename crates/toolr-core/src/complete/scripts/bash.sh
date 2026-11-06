# shellcheck shell=bash
# toolr bash completion - delegates to `toolr __complete`. Needs bash 4.4 or newer.
#
# Install via `toolr self completion install bash`, or source this file
# directly. Re-source on every shell start; manifest contents are read
# at Tab time, not when this script is sourced.
#
# Every local is `_toolr_`-prefixed: `${!name}` is dynamically scoped, so it would read a
# local of any function up the call stack that shares the user's variable name.

# Sets the caller's `_toolr_q` to `$1` quoted as one inert word, with no `eval`, for the open
# quote in the caller's `_toolr_qchar`: readline inserts the reply inside it and closes it.
_toolr_quote() {
    local _toolr_bare_tilde
    case ${_toolr_qchar-} in
        \")
            _toolr_q=${1//\\/\\\\}
            _toolr_q=${_toolr_q//\"/\\\"}
            _toolr_q=${_toolr_q//\$/\\\$}
            _toolr_q=${_toolr_q//\`/\\\`}
            return 0
            ;;
        \')
            _toolr_q=${1//\'/\'\\\'\'}
            return 0
            ;;
    esac
    printf -v _toolr_q '%q' "$1"
    # Bash < 5 `%q` leaves `~` bare, and bash tilde-expands it at a word start or after `=`.
    printf -v _toolr_bare_tilde '%q' '~'
    if [[ $_toolr_bare_tilde == '~' ]]; then
        _toolr_q=${_toolr_q//\~/\\~}
    fi
}

# Sets the caller's `_toolr_q` to `$2` with the leading `$1` stripped, quoted by `_toolr_quote`.
_toolr_quote_after() {
    _toolr_quote "${2#"$1"}"
}

_toolr_complete() {
    # Bash splits COMP_WORDS at COMP_WORDBREAKS (`a:b` is `a`, `:`, `b`): glue the
    # pieces back together where COMP_LINE has no whitespace between them.
    local _toolr_line=${COMP_LINE:0:COMP_POINT} _toolr_words=() _toolr_n=0
    local _toolr_i _toolr_w _toolr_ws
    for ((_toolr_i = 0; _toolr_i <= COMP_CWORD; _toolr_i++)); do
        _toolr_ws=${_toolr_line%%[![:space:]]*}
        _toolr_line=${_toolr_line#"$_toolr_ws"}
        if ((_toolr_i == COMP_CWORD)); then
            # A cursor inside a word completes only what precedes it.
            _toolr_w=$_toolr_line
        else
            _toolr_w=${COMP_WORDS[_toolr_i]}
            _toolr_line=${_toolr_line#"$_toolr_w"}
        fi
        if ((_toolr_i > 0)) && [[ -z $_toolr_ws ]]; then
            _toolr_words[_toolr_n - 1]+=$_toolr_w
        else
            _toolr_words[_toolr_n]=$_toolr_w
            _toolr_n=$((_toolr_n + 1))
        fi
    done
    # The word as bash shows it, backslash-escaped.
    local _toolr_cur=${_toolr_words[_toolr_n - 1]}

    # A prefix bash inserted earlier arrives backslash-escaped (`dry\ `), or quoted
    # (`"dry r`), while the engine matches raw values. Readline replaces only the text after
    # the last unquoted wordbreak, or after a quote still open at the cursor, so keep the raw
    # text up to there. `$` and `@` are bash's readline special prefixes: the replaced text
    # starts at them, so they stay in the reply.
    local _toolr_breaks=${COMP_WORDBREAKS-}
    _toolr_breaks=${_toolr_breaks//[$' \t\n\'"']/}
    # `_toolr_raw_cur` is `_toolr_cur` dequoted; `_toolr_raw_prefix` the part of it readline
    # keeps; `_toolr_qchar` the quote still open at the cursor, if any.
    local _toolr_raw_cur='' _toolr_raw_prefix='' _toolr_c _toolr_qchar='' _toolr_before_quote=''
    for ((_toolr_i = 0; _toolr_i < ${#_toolr_cur}; _toolr_i++)); do
        _toolr_c=${_toolr_cur:_toolr_i:1}
        if [[ $_toolr_qchar == "'" ]]; then
            if [[ $_toolr_c == "'" ]]; then
                _toolr_qchar=
            else
                _toolr_raw_cur+=$_toolr_c
            fi
            continue
        fi
        if [[ $_toolr_qchar == '"' ]]; then
            if [[ $_toolr_c == '"' ]]; then
                _toolr_qchar=
                continue
            fi
            # Inside `"…"` a backslash escapes only these four.
            if [[ $_toolr_c == \\ && ${_toolr_cur:_toolr_i+1:1} == [\$\`\"\\] ]]; then
                _toolr_i=$((_toolr_i + 1))
                _toolr_c=${_toolr_cur:_toolr_i:1}
            fi
            _toolr_raw_cur+=$_toolr_c
            continue
        fi
        if [[ $_toolr_c == \\ ]]; then
            _toolr_i=$((_toolr_i + 1))
            _toolr_raw_cur+=${_toolr_cur:_toolr_i:1}
            continue
        fi
        if [[ $_toolr_c == [\'\"] ]]; then
            _toolr_qchar=$_toolr_c
            _toolr_before_quote=$_toolr_raw_cur
            continue
        fi
        _toolr_raw_cur+=$_toolr_c
        # `-n $_toolr_breaks` guards: an empty pattern `*""*` matches every character.
        if [[ -n $_toolr_breaks && $_toolr_breaks == *"$_toolr_c"* ]]; then
            _toolr_raw_prefix=$_toolr_raw_cur
            if [[ $_toolr_c == [\$@] ]]; then
                _toolr_raw_prefix=${_toolr_raw_cur%?}
            fi
        fi
    done
    [[ -z $_toolr_qchar ]] || _toolr_raw_prefix=$_toolr_before_quote

    # Words after the cursor are dropped; `_toolr_raw_cur` stands in for the cursor's word.
    local _toolr_args=()
    for ((_toolr_i = 1; _toolr_i < _toolr_n - 1; _toolr_i++)); do
        _toolr_args+=("${_toolr_words[_toolr_i]}")
    done
    _toolr_args+=("$_toolr_raw_cur")

    # `|| _toolr_rc=$?` keeps a non-zero exit from tripping the user's `set -e`.
    local _toolr_raw _toolr_rc=0
    _toolr_raw=$(toolr __complete "$PWD" "${_toolr_args[@]}" 2>/dev/null) || _toolr_rc=$?

    # `--opt=value`: the engine completes the part after the first `=`; `--opt=` is put back here.
    local _toolr_opt_prefix=''
    if [[ $_toolr_raw_cur == --?*=* ]]; then
        _toolr_opt_prefix=${_toolr_raw_cur%%=*}=
    fi
    # 10 = COMPLETE_PATHS, 11 = COMPLETE_DIRECTORIES in crates/toolr/src/dispatch.rs.
    case $_toolr_rc in
        0) ;;
        10 | 11)
            # Paths complete the part after `--opt=`. With `=` not in COMP_WORDBREAKS readline
            # replaces the whole word, so each reply must start with the option prefix again.
            # Without `--opt=`, `_toolr_opt_prefix` is empty and the word passes through as is.
            local _toolr_path_cur=${_toolr_raw_cur#"$_toolr_opt_prefix"}
            local _toolr_path_prefix='' _toolr_lead=''
            if [[ $_toolr_raw_prefix == "$_toolr_opt_prefix"* ]]; then
                _toolr_path_prefix=${_toolr_raw_prefix#"$_toolr_opt_prefix"}
            else
                _toolr_lead=${_toolr_opt_prefix#"$_toolr_raw_prefix"}
                _toolr_path_prefix=
            fi
            local _toolr_mode=-f
            ((_toolr_rc == 11)) && _toolr_mode=-d
            # Only an unquoted leading `~`, or a `$` bare or just inside `"`, expands.
            local _toolr_path_word=${_toolr_cur#"$_toolr_opt_prefix"} _toolr_tilde_ok=0 _toolr_var_ok=0
            [[ $_toolr_path_word == '~'* ]] && _toolr_tilde_ok=1
            [[ $_toolr_path_word == '$'* || $_toolr_path_word == '"$'* ]] && _toolr_var_ok=1
            _toolr_complete_paths "$_toolr_mode" "$_toolr_path_cur" "$_toolr_path_prefix" "$_toolr_lead" \
                "$_toolr_qchar" "$_toolr_tilde_ok" "$_toolr_var_ok"
            return 0
            ;;
        *) return 0 ;;
    esac
    [[ -z "$_toolr_raw" ]] && return 0

    # Each line is `value<TAB>description` - bash has no per-candidate
    # description support, so drop everything from the tab onward.
    # Candidates are repo-derived data: `compgen -W` would expand `$(...)`
    # inside a value, so compare literally and quote with `printf %q`.
    local _toolr_value _toolr_q _toolr_full
    while IFS=$'\t' read -r _toolr_value _; do
        _toolr_full=$_toolr_opt_prefix$_toolr_value
        _toolr_quote "$_toolr_full"
        # `_toolr_cur` may already hold the quoted form from an earlier insertion.
        if [[ $_toolr_full == "$_toolr_raw_cur"* || $_toolr_q == "$_toolr_cur"* ]]; then
            [[ -n $_toolr_raw_prefix ]] && _toolr_quote_after "$_toolr_raw_prefix" "$_toolr_full"
            COMPREPLY+=("$_toolr_q")
        fi
    done <<< "$_toolr_raw"
}

# Prints each path that starts with `$2`, NUL-terminated; MODE `$1` `-d` keeps only directories.
# The typed text is a quoted part of the pattern, so it matches literally: nothing in it is
# dequoted, expanded or run, unlike `compgen`, which dequotes it again inside a live completion.
# Run it in a subshell: it sets glob options for itself.
_toolr_list_paths() {
    local _toolr_dir=${2%"${2##*/}"} _toolr_leaf=${2##*/} _toolr_name
    set +f
    shopt -s nullglob dotglob
    shopt -u failglob nocaseglob
    unset GLOBIGNORE
    for _toolr_name in "$2"*; do
        # Bash 5.2's `globskipdots` and older bash disagree on these; they're added below.
        case ${_toolr_name#"$_toolr_dir"} in
            . | ..) continue ;;
        esac
        if [[ $1 == -d && ! -d $_toolr_name ]]; then
            continue
        fi
        printf '%s\0' "$_toolr_name"
    done
    # `compgen -f` offers `.` and `..` once the typed name starts with a dot.
    for _toolr_name in . ..; do
        if [[ $_toolr_leaf == .* && $_toolr_name == "$_toolr_leaf"* ]]; then
            printf '%s\0' "$_toolr_dir$_toolr_name"
        fi
    done
}

# File names are data too: each is quoted like a candidate value.
# Usage: _toolr_complete_paths MODE RAW_CUR [RAW_PREFIX [LEAD [QCHAR [TILDE_OK [VAR_OK]]]]], where
# MODE is `-f` or `-d`, RAW_CUR the dequoted typed path, RAW_PREFIX the part of it readline keeps,
# LEAD text readline replaces in front of the path (`--opt=`), which every reply starts with,
# QCHAR the quote open at the cursor, and TILDE_OK / VAR_OK 1 when a leading `~` / `$` expands.
_toolr_complete_paths() {
    local _toolr_mode=$1 _toolr_raw_cur=$2 _toolr_raw_prefix=${3-} _toolr_lead=${4-}
    local _toolr_qchar=${5-} _toolr_tilde_ok=${6-0} _toolr_var_ok=${7-0}
    local _toolr_name _toolr_rest _toolr_q _toolr_keep _toolr_is_dir _toolr_dirs=0
    local _toolr_bare='' _toolr_var='' _toolr_value='' _toolr_user_tilde=0
    local -A _toolr_tilde_dirs=()
    # shellcheck disable=SC2016 # The `$` is literal: a typed variable, not one to expand here.
    local _toolr_var_re='^(\$([A-Za-z_][A-Za-z0-9_]*)|\$\{([A-Za-z_][A-Za-z0-9_]*)\})/'
    local _toolr_user_re='^~([A-Za-z0-9._][A-Za-z0-9._-]*)?(/|$)'
    # `${#COMPREPLY[@]}` errors under `set -u` while the array is unset.
    COMPREPLY=()
    # A leading `$NAME/` or `~/` stays as typed, like native bash; only its value is listed and
    # tested. `${!_toolr_var}` reads a validated name, so nothing else in the word is expanded.
    # `$_` is excluded: inside this function it is the last argument, not the user's.
    # An excluded or unmatched `$` form falls through to the guard below, which drops it.
    if ((_toolr_var_ok)) && [[ $_toolr_raw_cur =~ $_toolr_var_re ]]; then
        _toolr_bare=${BASH_REMATCH[1]}
        _toolr_var=${BASH_REMATCH[2]}${BASH_REMATCH[3]}
        [[ $_toolr_var != _ ]] || _toolr_bare=
    elif ((_toolr_tilde_ok)) && [[ $_toolr_raw_cur == \~/* && -n ${HOME+set} ]]; then
        _toolr_bare='~'
        _toolr_var=HOME
    fi
    # Readline keeps no text, or text starting at the prefix; otherwise the word isn't ours to rewrite.
    if [[ -n $_toolr_bare && ( -z $_toolr_raw_prefix || $_toolr_raw_prefix == "$_toolr_bare"/* ) ]]; then
        [[ -n ${!_toolr_var+set} ]] || return 0
        _toolr_value=${!_toolr_var}
        _toolr_raw_cur=$_toolr_value${_toolr_raw_cur#"$_toolr_bare"}
        if [[ -n $_toolr_raw_prefix ]]; then
            _toolr_raw_prefix=$_toolr_value${_toolr_raw_prefix#"$_toolr_bare"}
        fi
    else
        _toolr_bare=
    fi
    # Bash 5.3 `compgen` expands a `$` in the word, running any `$(...)` or backtick inside
    # `${...}`. Paths are listed by glob now, but the guard stays: a word still holding either,
    # typed or from the value, offers nothing.
    [[ $_toolr_raw_cur != *[\$\`]* ]] || return 0
    # `~user` (or `~` with `HOME` unset) needs `compgen` to find the home directory. Readline's
    # dequoting leaves the word alone only if it holds no backslash or quote, so take only those.
    if [[ -z $_toolr_bare && $_toolr_tilde_ok == 1 && $_toolr_raw_cur == '~'* ]]; then
        if [[ ! $_toolr_raw_cur =~ $_toolr_user_re || $_toolr_raw_cur == *[\\\'\"]* ]]; then
            return 0
        fi
        _toolr_user_tilde=1
        # `[[ -d ]]` can't test `~user/...` without expanding it, but `compgen -d` resolves it.
        # `|| :` because `compgen` exits 1 on no match, which `set -e` would act on.
        if [[ $_toolr_mode == -f ]]; then
            while IFS= read -r _toolr_name; do
                _toolr_tilde_dirs[$_toolr_name]=1
            done < <(compgen -d -- "$_toolr_raw_cur" || :)
        fi
    fi
    while IFS= read -r -d '' _toolr_name; do
        _toolr_keep=
        _toolr_q=
        if [[ -n $_toolr_raw_prefix ]]; then
            if [[ $_toolr_name != "$_toolr_raw_prefix" ]]; then
                _toolr_quote_after "$_toolr_raw_prefix" "$_toolr_name"
            fi
        else
            _toolr_rest=$_toolr_name
            if [[ -n $_toolr_bare ]]; then
                # Put the typed `$NAME` or `~` back in place of the value the glob listed under.
                _toolr_keep=$_toolr_bare
                _toolr_rest=${_toolr_name#"$_toolr_value"}
            elif ((_toolr_user_tilde)) && [[ $_toolr_name == '~'* ]]; then
                # `compgen` echoes a typed leading `~`; it must stay bare to expand.
                _toolr_keep='~'
                _toolr_rest=${_toolr_name#\~}
            fi
            [[ -n $_toolr_rest ]] && _toolr_quote "$_toolr_rest"
        fi
        _toolr_q=$_toolr_lead$_toolr_keep$_toolr_q
        _toolr_is_dir=0
        if [[ $_toolr_mode == -d ]]; then
            _toolr_is_dir=1
        elif ((_toolr_user_tilde)); then
            [[ -n ${_toolr_tilde_dirs[$_toolr_name]-} ]] && _toolr_is_dir=1
        else
            # A test on a listed name, so nothing is expanded.
            [[ -d $_toolr_name ]] && _toolr_is_dir=1
        fi
        if ((_toolr_is_dir)); then
            _toolr_q+=/
            _toolr_dirs=$((_toolr_dirs + 1))
        fi
        COMPREPLY+=("$_toolr_q")
    done < <(
        if ((_toolr_user_tilde)); then
            compgen "$_toolr_mode" -- "$_toolr_raw_cur" | while IFS= read -r _toolr_name; do
                printf '%s\0' "$_toolr_name"
            done
        else
            _toolr_list_paths "$_toolr_mode" "$_toolr_raw_cur"
        fi
    )
    if [[ $_toolr_mode == -d ]] || ((${#COMPREPLY[@]} == 1 && _toolr_dirs == 1)); then
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
