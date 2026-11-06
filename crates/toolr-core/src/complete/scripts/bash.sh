# shellcheck shell=bash
# toolr bash completion - delegates to `toolr __complete`. Needs bash 4.4 or newer.
#
# Install via `toolr self completion install bash`, or source this file
# directly. Re-source on every shell start; manifest contents are read
# at Tab time, not when this script is sourced.
#
# Every local is `_toolr_`-prefixed: `${!name}` is dynamically scoped, so it would read a
# local of any function up the call stack that shares the user's variable name.

# Sets the caller's `_toolr_e` to the `$'…'` escape for the control character `$1`: the inside
# of its `printf %q` form, which escapes each byte, so a C1 control in UTF-8 becomes two.
_toolr_ansi_c_char() {
    printf -v _toolr_e '%q' "$1"
    _toolr_e=${_toolr_e#\$\'}
    _toolr_e=${_toolr_e%\'}
}

# Sets the caller's `_toolr_q` to `$1` quoted as one inert word, with no `eval`. Reads the
# caller's `_toolr_qchar`, the quote open at the cursor: readline inserts the reply inside it.
# Sets `_toolr_qextra` to 1 when the reply ends in an extra quote character that closes it.
_toolr_quote() {
    local _toolr_i _toolr_c _toolr_e _toolr_out
    _toolr_qextra=0
    case ${_toolr_qchar-} in
        \")
            _toolr_q=${1//\\/\\\\}
            _toolr_q=${_toolr_q//\"/\\\"}
            _toolr_q=${_toolr_q//\$/\\\$}
            _toolr_q=${_toolr_q//\`/\\\`}
            # Interactive bash history-expands `!` even inside `"…"`, and `\!` there would keep
            # its backslash, so close the quote, escape the `!` outside it, and reopen.
            if [[ $- == *H* ]]; then
                _toolr_q=${_toolr_q//!/\"\\!\"}
            fi
            ;;
        \')
            _toolr_q=${1//\'/\'\\\'\'}
            ;;
        \$\')
            # Bash doesn't history-expand a `!` inside `$'…'`, so only these two need escaping.
            # Readline sees the run as `'…'`, where `\'` would close it: `'` is `\047` instead.
            _toolr_q=${1//\\/\\\\}
            _toolr_q=${_toolr_q//\'/\\047}
            ;;
        *)
            _toolr_quote_bare "$1"
            return 0
            ;;
    esac
    # A control character can't be inserted raw: inside `$'…'` it's an escape, and inside `"…"`
    # or `'…'` the quote closes around a `$'…'` run of it and reopens.
    if [[ $_toolr_q == *[[:cntrl:]]* ]]; then
        _toolr_out=
        for ((_toolr_i = 0; _toolr_i < ${#_toolr_q}; _toolr_i++)); do
            _toolr_c=${_toolr_q:_toolr_i:1}
            if [[ $_toolr_c == [[:cntrl:]] ]]; then
                _toolr_ansi_c_char "$_toolr_c"
                _toolr_c=$_toolr_e
                [[ $_toolr_qchar == \$\' ]] || _toolr_c=$_toolr_qchar\$\'$_toolr_c\'$_toolr_qchar
            fi
            _toolr_out+=$_toolr_c
        done
        _toolr_q=$_toolr_out
    fi
    # Readline's `insert_match` lets a reply starting with the quote the user opened overwrite
    # that quote rather than double it, so start the reply with two.
    if [[ ${_toolr_q:0:1} == "$_toolr_qchar" ]]; then
        _toolr_q=$_toolr_qchar$_toolr_q
    fi
    # Readline adds no closing quote to a reply that already ends in one, so add one more. A
    # `$'` reply never ends in `'`.
    if [[ -n $_toolr_q && ${_toolr_q: -1} == "${_toolr_qchar: -1}" ]]; then
        _toolr_q+=${_toolr_qchar: -1}
        _toolr_qextra=1
    fi
}

# `_toolr_quote` for no open quote: `$1` as one `printf %q` word.
_toolr_quote_bare() {
    local _toolr_bare_tilde _toolr_i _toolr_e
    printf -v _toolr_q '%q' "$1"
    # `%q` quotes a whole word holding a control character as one `$'…'` run. Quote each
    # character alone instead, so the common prefix readline inserts can't end inside a run.
    if [[ $_toolr_q == *\$\'* ]]; then
        _toolr_q=
        for ((_toolr_i = 0; _toolr_i < ${#1}; _toolr_i++)); do
            printf -v _toolr_e '%q' "${1:_toolr_i:1}"
            _toolr_q+=$_toolr_e
        done
    fi
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

# Decodes the `$'…'` escape whose backslash is at `$1`'s offset `_toolr_i`: appends it to the
# caller's `_toolr_raw_cur` and moves `_toolr_i` to its last character. Returns 1 for an escape
# it declines: one bash keeps as typed, `\cX`, `\'`, NUL, or `\u` the locale can't encode.
_toolr_ansi_c_escape() {
    local _toolr_e=${1:_toolr_i+1:1} _toolr_n _toolr_digits _toolr_out
    _toolr_i=$((_toolr_i + 1))
    case $_toolr_e in
        # A lone `\` ending the word is an escape still being typed.
        '') return 0 ;;
        a) _toolr_out=$'\a' ;;
        b) _toolr_out=$'\b' ;;
        e | E) _toolr_out=$'\e' ;;
        f) _toolr_out=$'\f' ;;
        n) _toolr_out=$'\n' ;;
        r) _toolr_out=$'\r' ;;
        t) _toolr_out=$'\t' ;;
        v) _toolr_out=$'\v' ;;
        \\ | \" | \?) _toolr_out=$_toolr_e ;;
        # Readline sees the run as `'…'`, where this `'` closes it, so the two would disagree on
        # the open quote and readline would insert a reply quoted for the wrong one.
        \') return 1 ;;
        [0-7] | x | u | U)
            # The digits are matched first, so the formats below hold nothing else.
            case $_toolr_e in
                [0-7]) [[ ${1:_toolr_i} =~ ^[0-7]{1,3} ]] ;;
                x) [[ ${1:_toolr_i+1} =~ ^[0-9A-Fa-f]{1,2} ]] ;;
                u) [[ ${1:_toolr_i+1} =~ ^[0-9A-Fa-f]{1,4} ]] ;;
                *) [[ ${1:_toolr_i+1} =~ ^[0-9A-Fa-f]{1,8} ]] ;;
            esac || return 1
            _toolr_digits=${BASH_REMATCH[0]}
            if [[ $_toolr_e == [0-7] ]]; then
                _toolr_i=$((_toolr_i + ${#_toolr_digits} - 1))
                _toolr_n=$((8#$_toolr_digits))
            else
                _toolr_i=$((_toolr_i + ${#_toolr_digits}))
                _toolr_n=$((16#$_toolr_digits))
            fi
            # No name holds a NUL, and bash would cut the word at one.
            ((_toolr_n > 0)) || return 1
            if [[ $_toolr_e == [0-7x] ]]; then
                ((_toolr_n < 256)) || return 1
                printf -v _toolr_digits '%03o' "$_toolr_n"
                # shellcheck disable=SC2059 # The format is `\` and three octal digits.
                printf -v _toolr_out "\\$_toolr_digits"
            else
                # `\u` needs a UTF-8 locale, where this two-byte `é` is one character. Outside one,
                # bash's `printf` may leave it as typed or encode it in UTF-8 anyway.
                _toolr_out=$'\xc3\xa9'
                ((${#_toolr_out} == 1 && _toolr_n <= 0x10FFFF)) || return 1
                # shellcheck disable=SC2059 # The format is `\u` or `\U` and the hex digits.
                printf -v _toolr_out "\\$_toolr_e$_toolr_digits"
                [[ $_toolr_out != \\* ]] || return 1
            fi
            ;;
        *) return 1 ;;
    esac
    _toolr_raw_cur+=$_toolr_out
}

# Dequotes the word `$1` (`dry\ r`, `"dry r`, `$'dry\x20r`) for the engine, which matches raw
# values. Sets the caller's `_toolr_raw_cur` to the result, `_toolr_raw_prefix` to the part
# readline keeps (up to the last unquoted break, or an open quote), `_toolr_qchar` to the quote
# open at the cursor, and `_toolr_declined` to 1 for a `$'…'` escape it won't decode.
_toolr_parse_word() {
    local _toolr_breaks=${COMP_WORDBREAKS-} _toolr_i _toolr_c _toolr_before_quote=''
    # Blanks and quotes in the word are escaped or quoting, never breaks.
    _toolr_breaks=${_toolr_breaks//[$' \t\n\'"']/}
    _toolr_raw_cur=
    _toolr_raw_prefix=
    _toolr_qchar=
    _toolr_declined=0
    for ((_toolr_i = 0; _toolr_i < ${#1}; _toolr_i++)); do
        _toolr_c=${1:_toolr_i:1}
        if [[ $_toolr_qchar == \$\' ]]; then
            if [[ $_toolr_c == "'" ]]; then
                # Readline reads a `'` right after a `\` as escaped inside `$'…'`, though `\\'`
                # closes the run, so it would misplace the word's start.
                if [[ ${1:_toolr_i-1:1} == \\ ]]; then
                    _toolr_declined=1
                    return 0
                fi
                _toolr_qchar=
            elif [[ $_toolr_c == \\ ]]; then
                if ! _toolr_ansi_c_escape "$1"; then
                    _toolr_declined=1
                    return 0
                fi
            else
                _toolr_raw_cur+=$_toolr_c
            fi
            continue
        fi
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
            if [[ $_toolr_c == \\ && ${1:_toolr_i+1:1} == [\$\`\"\\] ]]; then
                _toolr_i=$((_toolr_i + 1))
                _toolr_c=${1:_toolr_i:1}
            fi
            _toolr_raw_cur+=$_toolr_c
            continue
        fi
        if [[ $_toolr_c == \\ ]]; then
            _toolr_i=$((_toolr_i + 1))
            _toolr_raw_cur+=${1:_toolr_i:1}
            continue
        fi
        if [[ $_toolr_c == [\'\"] ]]; then
            _toolr_qchar=$_toolr_c
            _toolr_before_quote=$_toolr_raw_cur
            continue
        fi
        # `$'` opens an ANSI-C run; its `$` is not part of the name. With `$` added to the
        # default `COMP_WORDBREAKS`, readline still replaces the whole word, so it isn't a break.
        if [[ $_toolr_c == \$ && ${1:_toolr_i+1:1} == "'" ]]; then
            _toolr_i=$((_toolr_i + 1))
            _toolr_qchar=\$\'
            _toolr_before_quote=$_toolr_raw_cur
            continue
        fi
        _toolr_raw_cur+=$_toolr_c
        # `-n $_toolr_breaks` guards: an empty pattern `*""*` matches every character.
        if [[ -n $_toolr_breaks && $_toolr_breaks == *"$_toolr_c"* ]]; then
            _toolr_raw_prefix=$_toolr_raw_cur
            # `$` and `@` are readline special prefixes: the replaced text starts at them.
            if [[ $_toolr_c == [\$@] ]]; then
                _toolr_raw_prefix=${_toolr_raw_cur%?}
            fi
        fi
    done
    # Readline replaces from just after a quote still open at the cursor.
    [[ -z $_toolr_qchar ]] || _toolr_raw_prefix=$_toolr_before_quote
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

    local _toolr_raw_cur _toolr_raw_prefix _toolr_qchar _toolr_declined
    # A `$'…'` form readline misreads, anywhere before the cursor, puts its quoting out of step
    # with the script's for the rest of the line.
    _toolr_parse_word "${COMP_LINE:0:COMP_POINT}"
    if ((_toolr_declined)); then
        return 0
    fi
    _toolr_parse_word "$_toolr_cur"
    if ((_toolr_declined)); then
        return 0
    fi
    # `insert-completions` (`M-*`) deletes the open quote and inserts each reply as is, unquoted.
    if [[ ${COMP_TYPE-} == 42 && -n $_toolr_qchar ]]; then
        return 0
    fi

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
            local _toolr_mode=-f
            ((_toolr_rc == 11)) && _toolr_mode=-d
            _toolr_complete_paths "$_toolr_mode" "$_toolr_opt_prefix"
            return 0
            ;;
        *) return 0 ;;
    esac
    [[ -z "$_toolr_raw" ]] && return 0

    # Each line is `value<TAB>description` - bash has no per-candidate
    # description support, so drop everything from the tab onward.
    # Candidates are repo-derived data: `compgen -W` would expand `$(...)`
    # inside a value, so compare literally and quote with `printf %q`.
    local _toolr_value _toolr_q _toolr_qextra _toolr_full
    while IFS=$'\t' read -r _toolr_value _; do
        _toolr_full=$_toolr_opt_prefix$_toolr_value
        _toolr_quote "$_toolr_full"
        # `_toolr_cur` may already hold the quoted form from an earlier insertion.
        if [[ $_toolr_full == "$_toolr_raw_cur"* || $_toolr_q == "$_toolr_cur"* ]]; then
            if [[ -n $_toolr_raw_prefix ]]; then
                # Nothing is left to insert after the kept text.
                [[ $_toolr_full != "$_toolr_raw_prefix" ]] || continue
                _toolr_quote_after "$_toolr_raw_prefix" "$_toolr_full"
            fi
            COMPREPLY+=("$_toolr_q")
        fi
    done <<< "$_toolr_raw"
}

# Prints each path that starts with `$2`, NUL-terminated; MODE `$1` `-d` keeps only directories.
# The typed text is a quoted part of the pattern, so it matches literally: nothing in it is
# dequoted, expanded or run, unlike `compgen`, which dequotes it again inside a live completion.
# Run it in a subshell: it sets glob options for itself.
_toolr_list_paths() {
    local _toolr_mode=$1 _toolr_word=$2 _toolr_name
    local _toolr_dir=${_toolr_word%"${_toolr_word##*/}"} _toolr_leaf=${_toolr_word##*/}
    # The user's `set -f` would turn the glob below off.
    set +f
    # First: unsetting `GLOBIGNORE` also turns `dotglob` off.
    unset GLOBIGNORE
    # `dotglob` lists dotfiles with no `.` typed, as `compgen -f` did.
    shopt -s nullglob dotglob
    shopt -u failglob nocaseglob
    for _toolr_name in "$_toolr_word"*; do
        # Bash 5.2's `globskipdots` and older bash disagree on these, so they're added below.
        case ${_toolr_name#"$_toolr_dir"} in
            . | ..) continue ;;
        esac
        if [[ $_toolr_mode == -d && ! -d $_toolr_name ]]; then
            continue
        fi
        # NUL-terminated, so a name holding a newline stays one candidate.
        printf '%s\0' "$_toolr_name"
    done
    # `compgen -f` offers `.` and `..` once the typed name starts with a dot, the same on any bash.
    # Only under a directory that exists, as `compgen -f` did.
    [[ -d ${_toolr_dir:-.} ]] || return 0
    for _toolr_name in . ..; do
        if [[ $_toolr_leaf == .* && $_toolr_name == "$_toolr_leaf"* ]]; then
            printf '%s\0' "$_toolr_dir$_toolr_name"
        fi
    done
}

# Prints each path `compgen` lists for the `~user/...` word `$2`, NUL-terminated; MODE `$1`
# as for `compgen`. Only `compgen` resolves `~user`; the caller takes only words readline's
# dequoting leaves alone.
_toolr_list_user_paths() {
    local _toolr_name
    # `compgen` prints one name per line, so the tail of a name holding a newline doesn't start
    # with the typed `~user` and is dropped.
    compgen "$1" -- "$2" | while IFS= read -r _toolr_name; do
        [[ $_toolr_name != "${2%%/*}"* ]] || printf '%s\0' "$_toolr_name"
    done
}

# File names are data too: each is quoted like a candidate value.
# Usage: _toolr_complete_paths MODE OPT_PREFIX, where MODE is `-f` or `-d` and OPT_PREFIX the
# `--opt=` in front of the path, or empty. Reads the caller's `_toolr_cur` and the
# `_toolr_raw_cur`, `_toolr_raw_prefix` and `_toolr_qchar` that `_toolr_parse_word` set.
_toolr_complete_paths() {
    local _toolr_mode=$1 _toolr_opt_prefix=$2
    # The path is the part after `--opt=`. With `=` not in COMP_WORDBREAKS readline replaces the
    # whole word, so `_toolr_lead`, the part of `--opt=` it replaces, starts every reply.
    local _toolr_raw_cur=${_toolr_raw_cur#"$_toolr_opt_prefix"} _toolr_raw_prefix=$_toolr_raw_prefix
    local _toolr_lead=''
    if [[ $_toolr_raw_prefix == "$_toolr_opt_prefix"* ]]; then
        _toolr_raw_prefix=${_toolr_raw_prefix#"$_toolr_opt_prefix"}
    else
        _toolr_lead=${_toolr_opt_prefix#"$_toolr_raw_prefix"}
        _toolr_raw_prefix=
    fi
    # Read from the typed word, not the decoded one, where a `$` or `~` may come from `$'…'`.
    # A leading `~` expands only with nothing quoted before the first `/`, and a variable only
    # with its `$` and whole name typed bare or just inside `"`.
    local _toolr_path_word=${_toolr_cur#"$_toolr_opt_prefix"} _toolr_tilde_ok=0 _toolr_typed_var=''
    # shellcheck disable=SC2016 # The `$` is literal: a typed variable, not one to expand here.
    local _toolr_typed_var_re='^"?(\$[A-Za-z_][A-Za-z0-9_]*|\$\{[A-Za-z_][A-Za-z0-9_]*\})'
    if [[ $_toolr_path_word == '~'* && ${_toolr_path_word%%/*} != *[\\\'\"\$]* ]]; then
        _toolr_tilde_ok=1
    fi
    if [[ $_toolr_path_word =~ $_toolr_typed_var_re ]]; then
        _toolr_typed_var=${BASH_REMATCH[1]}
    fi
    local _toolr_name _toolr_rest _toolr_q _toolr_qextra _toolr_keep _toolr_is_dir _toolr_dirs=0
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
    if [[ -n $_toolr_typed_var && $_toolr_raw_cur =~ $_toolr_var_re ]]; then
        _toolr_bare=${BASH_REMATCH[1]}
        _toolr_var=${BASH_REMATCH[2]}${BASH_REMATCH[3]}
        [[ $_toolr_var != _ && $_toolr_bare == "$_toolr_typed_var" ]] || _toolr_bare=
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
    # `${...}`. The guard is kept on purpose so no `$` or backtick ever reaches a listing, typed
    # or from the value.
    [[ $_toolr_raw_cur != *[\$\`]* ]] || return 0
    # `~user` (or `~` with `HOME` unset) needs `compgen` to find the home directory. Readline's
    # dequoting leaves the word alone only if it holds no backslash or quote, so take only those.
    if [[ -z $_toolr_bare && $_toolr_raw_cur == '~'* ]] && ((_toolr_tilde_ok)); then
        if [[ ! $_toolr_raw_cur =~ $_toolr_user_re || $_toolr_raw_cur == *[\\\'\"]* ]]; then
            return 0
        fi
        _toolr_user_tilde=1
        # `[[ -d ]]` can't test `~user/...` without expanding it, but `compgen -d` resolves it.
        if [[ $_toolr_mode == -f ]]; then
            while IFS= read -r -d '' _toolr_name; do
                _toolr_tilde_dirs[$_toolr_name]=1
            done < <(_toolr_list_user_paths -d "$_toolr_raw_cur")
        fi
    fi
    # `read -d ''` splits at the NULs the listers print, so a name with a newline is one candidate.
    while IFS= read -r -d '' _toolr_name; do
        _toolr_keep=
        _toolr_q=
        _toolr_qextra=0
        if [[ -n $_toolr_raw_prefix ]]; then
            if [[ $_toolr_name != "$_toolr_raw_prefix" ]]; then
                _toolr_quote_after "$_toolr_raw_prefix" "$_toolr_name"
            fi
        else
            _toolr_rest=$_toolr_name
            if [[ -n $_toolr_bare ]]; then
                # Put the typed `$NAME/` or `~/` back in place of the value the glob listed under,
                # bare: a `~` followed by `$'…'` would not expand.
                _toolr_keep=$_toolr_bare/
                _toolr_rest=${_toolr_name#"$_toolr_value"/}
            elif ((_toolr_user_tilde)) && [[ $_toolr_name == '~'* ]]; then
                # `compgen` echoes a typed leading `~`; it must stay bare to expand.
                _toolr_keep=${_toolr_name%%/*}
                _toolr_rest=${_toolr_name#"$_toolr_keep"}
                if [[ $_toolr_rest == /* ]]; then
                    _toolr_keep+=/
                    _toolr_rest=${_toolr_rest#/}
                fi
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
        # A name equal to the kept text leaves nothing after the open quote but a directory's `/`.
        if [[ -n $_toolr_raw_prefix && $_toolr_name == "$_toolr_raw_prefix" ]] && ((!_toolr_is_dir)); then
            continue
        fi
        if ((_toolr_is_dir)); then
            # Readline closes the quote after the `/`, so the extra quote closing the reply must go.
            ((!_toolr_qextra)) || _toolr_q=${_toolr_q%?}
            _toolr_q+=/
            _toolr_dirs=$((_toolr_dirs + 1))
        fi
        COMPREPLY+=("$_toolr_q")
    done < <(
        if ((_toolr_user_tilde)); then
            _toolr_list_user_paths "$_toolr_mode" "$_toolr_raw_cur"
        else
            _toolr_list_paths "$_toolr_mode" "$_toolr_raw_cur"
        fi
    )
    if [[ $_toolr_mode == -d ]] || ((${#COMPREPLY[@]} == 1 && _toolr_dirs == 1)); then
        # `compopt` fails outside a live completion, such as a direct call.
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
