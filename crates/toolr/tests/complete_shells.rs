//! Completion scripts must treat repo-derived candidate values as data.
//! A `Literal[...]` value such as `$(touch X)` must never execute on TAB.
#![cfg(unix)]

mod common;

use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tempfile::TempDir;

/// Files under the fixture root that path slots complete. `files/sub/` is a directory.
const PATH_FILES: [&str; 3] = [
    "files/with space.txt",
    "files/$(touch sentinel-three)",
    "files/back\\slash",
];

/// `SENTINEL` and `SENTINEL2` stand for paths the fixture fills in.
const CHOICES: [&str; 7] = [
    "fast",
    "dry run",
    "dry walk",
    "$(touch SENTINEL)",
    "`touch SENTINEL2`",
    "~root",
    "x=~root",
];

struct Fixture {
    tmp: TempDir,
    sentinel: PathBuf,
    sentinel2: PathBuf,
    /// Created only if the file named `$(touch sentinel-three)` runs.
    sentinel3: PathBuf,
}

impl Fixture {
    fn choices(&self) -> Vec<String> {
        CHOICES
            .iter()
            .map(|c| {
                c.replace("SENTINEL2", self.sentinel2.to_str().unwrap())
                    .replace("SENTINEL", self.sentinel.to_str().unwrap())
            })
            .collect()
    }
}

fn fixture() -> Fixture {
    let tmp = TempDir::new().unwrap();
    let sentinel = tmp.path().join("sentinel-one");
    let sentinel2 = tmp.path().join("sentinel-two");
    let sentinel3 = tmp.path().join("sentinel-three");
    common::write_project(tmp.path());
    let files = tmp.path().join("files");
    std::fs::create_dir_all(files.join("sub")).unwrap();
    for name in PATH_FILES {
        std::fs::write(tmp.path().join(name), "").unwrap();
    }
    let fx = Fixture {
        tmp,
        sentinel,
        sentinel2,
        sentinel3,
    };
    let literal = fx
        .choices()
        .iter()
        .map(|c| format!("{c:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let src = format!(
        r#""""CI utilities."""
import pathlib
from typing import Literal

from toolr.types import DirectoryPath

group = command_group("ci", "CI utilities", docstring=__doc__)

@group.command
def lint(
    ctx,
    mode: Literal[{literal}] = "fast",
    cfg: pathlib.Path | None = None,
    out: DirectoryPath | None = None,
):
    """Lint things.

    Args:
        mode: Lint mode.
        cfg: Config path.
        out: Output directory.
    """
    pass
"#
    );
    std::fs::write(fx.tmp.path().join("tools/ci.py"), src).unwrap();

    common::toolr()
        .current_dir(fx.tmp.path())
        .arg("__build-static-manifest")
        .assert()
        .success();
    fx
}

fn toolr_dir() -> PathBuf {
    assert_cmd::cargo::cargo_bin("toolr")
        .parent()
        .unwrap()
        .to_path_buf()
}

fn print_script(shell: &str) -> String {
    let out = common::toolr()
        .args(["self", "completion", "print", shell])
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

fn path_with_toolr() -> String {
    format!(
        "{}:{}",
        toolr_dir().display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

struct BashRun {
    rc: String,
    stderr: String,
    reply: Vec<String>,
    quoted: Vec<String>,
    /// Arguments of each `compopt` call, recorded only where bash has the builtin.
    compopt: Vec<String>,
}

impl BashRun {
    /// The `printf %q` form of a [`CHOICES`] entry.
    fn quoted(&self, choice: &str) -> String {
        let i = CHOICES.iter().position(|c| *c == choice).unwrap();
        self.quoted[i].clone()
    }
}

/// Values reach bash only as positional parameters, never in the command string.
fn run_bash(bash: &str, fx: &Fixture, cur: &str) -> BashRun {
    run_bash_with_tail(bash, fx, cur, "")
}

/// `tail` holds words typed after the cursor's word, so `cword` is not last.
fn run_bash_with_tail(bash: &str, fx: &Fixture, cur: &str, tail: &str) -> BashRun {
    let script = print_script("bash");
    run_bash_script(bash, &script, fx, "--mode", cur, tail, &fx.choices())
}

/// Complete `cur` as the value of a path flag; `quote` lists the raw names
/// whose `%q` forms land in [`BashRun::quoted`].
fn run_bash_path(bash: &str, script: &str, fx: &Fixture, flag: &str, cur: &str) -> BashRun {
    let quote: Vec<String> = PATH_FILES.iter().map(|f| (*f).to_string()).collect();
    run_bash_script(bash, script, fx, flag, cur, "", &quote)
}

fn run_bash_script(
    bash: &str,
    script: &str,
    fx: &Fixture,
    flag: &str,
    cur: &str,
    tail: &str,
    quote: &[String],
) -> BashRun {
    const DRIVER: &str = r#"
script=$1; flag=$2; cur=$3; shift 3
if [ "$cur" = @QDRY ]; then cur=$(printf '%q' "dry "); fi
TW=(toolr ci lint "$flag" "$cur")
for t in $TW_TAIL; do TW+=("$t"); done
_init_completion() { words=("${TW[@]}"); cword=4; cur="${words[cword]}"; prev="${words[cword-1]}"; return 0; }
# The builtin only works inside a live completion, so record its calls instead.
if type compopt >/dev/null 2>&1; then compopt() { printf 'COMPOPT:%s\n' "$*"; }; fi
eval "$script"
_toolr_complete; rc=$?
printf 'RC:%s\n' "$rc"
for r in "${COMPREPLY[@]}"; do printf 'R:%s\n' "$r"; done
for v in "$@"; do printf 'Q:%s\n' "$(printf '%q' "$v")"; done
"#;
    let out = StdCommand::new(bash)
        .arg("-c")
        .arg(DRIVER)
        .arg("driver")
        .arg(script)
        .arg(flag)
        .arg(cur)
        .args(quote)
        .current_dir(fx.tmp.path())
        .env("PATH", path_with_toolr())
        // `~/` completes inside the fixture.
        .env("HOME", fx.tmp.path())
        // The shell runs toolr itself, so mirror `common::toolr`'s cache isolation.
        .env("XDG_CACHE_HOME", common::test_cache_home())
        .env("TW_TAIL", tail)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let pick = |p: &str| {
        stdout
            .lines()
            .filter_map(|l| l.strip_prefix(p).map(str::to_string))
            .collect::<Vec<_>>()
    };
    BashRun {
        rc: pick("RC:").concat(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        reply: pick("R:"),
        quoted: pick("Q:"),
        compopt: pick("COMPOPT:"),
    }
}

fn bash_has_compopt(bash: &str) -> bool {
    StdCommand::new(bash)
        .args(["-c", "type compopt"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn bashes() -> Vec<String> {
    let mut v = vec!["/bin/bash".to_string()];
    if let Ok(out) = StdCommand::new("sh")
        .args(["-c", "command -v bash"])
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .output()
    {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() && p != "/bin/bash" && Path::new(&p).exists() {
            v.push(p);
        }
    }
    v.retain(|p| Path::new(p).exists());
    v
}

#[test]
fn bash_empty_prefix_offers_every_value_quoted_and_inert() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash(&bash, &fx, "");
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert!(!fx.sentinel.exists(), "{bash}: $(...) value executed");
        assert!(!fx.sentinel2.exists(), "{bash}: backtick value executed");
        let expected = run.quoted("$(touch SENTINEL)");
        assert!(
            run.reply.contains(&expected),
            "{bash}: {expected:?} not in {:?}",
            run.reply
        );
        assert_eq!(run.reply.len(), CHOICES.len(), "{bash}: {:?}", run.reply);
    }
}

#[test]
fn bash_value_with_space_is_one_quoted_word() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash(&bash, &fx, "dry r");
        assert_eq!(run.reply, vec![run.quoted("dry run")], "{bash}");
    }
}

/// `%q` leaves `~` bare, which bash would tilde-expand at the start of a
/// word or after `=`.
#[test]
fn bash_tilde_is_escaped() {
    for bash in bashes() {
        let fx = fixture();
        for (cur, want) in [
            ("", "\\~root"),
            ("~r", "\\~root"),
            ("\\~r", "\\~root"),
            ("", "x=\\~root"),
            ("x=~", "x=\\~root"),
            ("x=\\~", "x=\\~root"),
        ] {
            let run = run_bash(&bash, &fx, cur);
            assert_eq!(run.rc, "0", "{bash} {cur:?}: {}", run.stderr);
            assert!(
                run.reply.contains(&want.to_string()),
                "{bash} {cur:?}: {:?}",
                run.reply
            );
            assert!(
                !run.reply.iter().any(|r| r.replace("\\~", "").contains('~')),
                "{bash} {cur:?}: bare tilde in {:?}",
                run.reply
            );
        }
        let run = run_bash(&bash, &fx, "fa");
        assert_eq!(run.reply, vec!["fast".to_string()], "{bash}");
    }
}

#[test]
fn bash_quoted_form_of_inserted_prefix_still_matches() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash(&bash, &fx, "@QDRY");
        assert_eq!(
            run.reply,
            vec![run.quoted("dry run"), run.quoted("dry walk")],
            "{bash}"
        );
    }
}

#[test]
fn bash_quoted_prefix_does_not_error() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash(&bash, &fx, "'fa");
        assert_eq!(run.rc, "0", "{bash}");
        assert_eq!(run.stderr, "", "{bash}");
    }
}

#[test]
fn bash_completes_the_cursor_word_when_it_is_not_last() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash_with_tail(&bash, &fx, "fa", "--verbose");
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert_eq!(run.reply, vec!["fast".to_string()], "{bash}");
    }
}

#[test]
fn bash_path_flag_completes_files_quoted_and_inert() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash_path(&bash, &print_script("bash"), &fx, "--cfg", "files/");
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert_eq!(run.stderr, "", "{bash}");
        let mut want = run.quoted.clone();
        want.push("files/sub/".to_string());
        want.sort();
        let mut got = run.reply.clone();
        got.sort();
        assert_eq!(got, want, "{bash}");
        assert!(!fx.sentinel3.exists(), "{bash}: file name executed");
        // Several matches: bash inserts only their common prefix, so no space to drop.
        assert!(run.compopt.is_empty(), "{bash}: {:?}", run.compopt);
    }
}

#[test]
fn bash_directory_flag_completes_only_directories() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash_path(&bash, &print_script("bash"), &fx, "--out", "files/");
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert_eq!(run.reply, vec!["files/sub/".to_string()], "{bash}");
        // Bash 3.2 has no `compopt` and keeps the space after `sub/`.
        let want: Vec<String> = if bash_has_compopt(&bash) {
            vec!["-o nospace".to_string()]
        } else {
            vec![]
        };
        assert_eq!(run.compopt, want, "{bash}");
    }
}

#[test]
fn bash_single_directory_match_on_a_path_flag_drops_the_space() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash_path(&bash, &print_script("bash"), &fx, "--cfg", "files/s");
        assert_eq!(run.reply, vec!["files/sub/".to_string()], "{bash}");
        let want: Vec<String> = if bash_has_compopt(&bash) {
            vec!["-o nospace".to_string()]
        } else {
            vec![]
        };
        assert_eq!(run.compopt, want, "{bash}");
        let run = run_bash_path(&bash, &print_script("bash"), &fx, "--cfg", "files/w");
        assert_eq!(
            run.reply,
            vec!["files/with\\ space.txt".to_string()],
            "{bash}"
        );
        assert!(run.compopt.is_empty(), "{bash}: {:?}", run.compopt);
    }
}

#[test]
fn bash_path_keeps_a_typed_leading_tilde() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash_path(&bash, &print_script("bash"), &fx, "--cfg", "~/files/");
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert!(
            run.reply.contains(&"~/files/sub/".to_string()),
            "{bash}: {:?}",
            run.reply
        );
        assert!(
            run.reply.contains(&"~/files/with\\ space.txt".to_string()),
            "{bash}: {:?}",
            run.reply
        );
        assert!(
            run.reply.iter().all(|r| r.starts_with("~/")),
            "{bash}: {:?}",
            run.reply
        );
        assert!(!fx.sentinel3.exists(), "{bash}: file name executed");
    }
}

/// A user's `set -e` must not leak into Tab: a non-zero `__complete` exit
/// would otherwise close their shell.
#[test]
fn bash_completion_survives_errexit() {
    for bash in bashes() {
        let fx = fixture();
        let script = format!("set -e\n{}", print_script("bash"));
        let run = run_bash_path(&bash, &script, &fx, "--out", "files/");
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert_eq!(run.reply, vec!["files/sub/".to_string()], "{bash}");
        let run = run_bash_path(&bash, &script, &fx, "--cfg", "files/s");
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert_eq!(run.reply, vec!["files/sub/".to_string()], "{bash}");
        let run = run_bash_script(&bash, &script, &fx, "--mode", "fa", "", &[]);
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert_eq!(run.reply, vec!["fast".to_string()], "{bash}");
    }
}

/// The login name and home directory `~user` expands to, which ignore `$HOME`.
fn passwd_entry() -> (String, PathBuf) {
    // SAFETY: getpwuid returns static storage, copied out before any other call.
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        assert!(!pw.is_null(), "no passwd entry");
        let name = CStr::from_ptr((*pw).pw_name).to_str().unwrap().to_string();
        let dir = CStr::from_ptr((*pw).pw_dir).to_str().unwrap().to_string();
        (name, PathBuf::from(dir))
    }
}

#[test]
fn bash_path_marks_a_tilde_user_directory() {
    let (user, home) = passwd_entry();
    let base = tempfile::Builder::new()
        .prefix(".toolr-complete-")
        .tempdir_in(&home)
        .unwrap();
    std::fs::create_dir(base.path().join("sub")).unwrap();
    let rel = base.path().strip_prefix(&home).unwrap().to_str().unwrap();
    let cur = format!("~{user}/{rel}/s");
    let want = format!("~{user}/{rel}/sub/");
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash_path(&bash, &print_script("bash"), &fx, "--cfg", &cur);
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert_eq!(run.reply, vec![want.clone()], "{bash}");
        let nospace: Vec<String> = if bash_has_compopt(&bash) {
            vec!["-o nospace".to_string()]
        } else {
            vec![]
        };
        assert_eq!(run.compopt, nospace, "{bash}");
    }
}

/// A typed `\\` stands for one literal backslash in the name.
#[test]
fn bash_path_unescapes_a_typed_backslash() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash_path(
            &bash,
            &print_script("bash"),
            &fx,
            "--cfg",
            "files/back\\\\s",
        );
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert_eq!(run.reply, vec![run.quoted[2].clone()], "{bash}");
        let run = run_bash_path(&bash, &print_script("bash"), &fx, "--cfg", "files/with\\ s");
        assert_eq!(run.reply, vec![run.quoted[0].clone()], "{bash}");
    }
}

/// The bash script shipped before path completion, as an installed copy would hold it.
const OLD_BASH_SCRIPT: &str = r#"# shellcheck shell=bash
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
    # while the engine matches raw values.
    local raw_cur=${cur//\\/}

    # Words after the cursor are dropped; `raw_cur` stands in for words[cword].
    local args=("${words[@]:1:cword-1}" "$raw_cur")

    local raw
    raw=$(toolr __complete "$PWD" "${args[@]}" 2>/dev/null) || return 0
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

complete -F _toolr_complete toolr
"#;

#[test]
fn old_bash_script_offers_nothing_on_a_path_slot() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash_path(&bash, OLD_BASH_SCRIPT, &fx, "--cfg", "files/");
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert_eq!(run.stderr, "", "{bash}");
        assert!(run.reply.is_empty(), "{bash}: {:?}", run.reply);
    }
}

fn have(shell: &str) -> bool {
    StdCommand::new(shell)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn fish_completion_never_executes_values() {
    if !have("fish") {
        return;
    }
    let fx = fixture();
    let script = print_script("fish");
    let out = StdCommand::new("fish")
        .arg("--no-config")
        .arg("-c")
        .arg("source (echo $argv[1] | psub); complete -C \"toolr ci lint --mode \"")
        .arg(script)
        .current_dir(fx.tmp.path())
        .env("PATH", path_with_toolr())
        // The shell runs toolr itself, so mirror `common::toolr`'s cache isolation.
        .env("XDG_CACHE_HOME", common::test_cache_home())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("fast"), "fish got no candidates: {stdout}");
    assert!(
        stdout.contains("$(touch"),
        "fish got no candidates: {stdout}"
    );
    assert!(!fx.sentinel.exists());
    assert!(!fx.sentinel2.exists());
}

fn fish_complete(fx: &Fixture, line: &str) -> String {
    let out = StdCommand::new("fish")
        .arg("--no-config")
        .arg("-c")
        .arg("source (echo $argv[1] | psub); complete -C $argv[2]")
        .arg(print_script("fish"))
        .arg(line)
        .current_dir(fx.tmp.path())
        .env("PATH", path_with_toolr())
        .env("XDG_CACHE_HOME", common::test_cache_home())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn fish_path_flag_completes_files_inert() {
    if !have("fish") {
        return;
    }
    let fx = fixture();
    let stdout = fish_complete(&fx, "toolr ci lint --cfg files/");
    let names: Vec<&str> = stdout
        .lines()
        .map(|l| l.split('\t').next().unwrap())
        .collect();
    for want in [
        "files/with space.txt",
        "files/sub/",
        "files/$(touch sentinel-three)",
    ] {
        assert!(names.contains(&want), "{want:?} not in {names:?}");
    }
    assert!(!fx.sentinel3.exists(), "fish executed a file name");

    let stdout = fish_complete(&fx, "toolr ci lint --out files/");
    let names: Vec<&str> = stdout
        .lines()
        .map(|l| l.split('\t').next().unwrap())
        .collect();
    assert_eq!(names, vec!["files/sub/"]);
}

/// A real zpty-driven run is not workable in a non-interactive harness, so
/// check the source: candidates must go through `compadd -a` without `-Q`,
/// which keeps zsh quoting them on insertion.
#[test]
fn zsh_completion_never_executes_values() {
    let script = print_script("zsh");
    let compadds: Vec<&str> = script
        .lines()
        .filter(|l| l.trim_start().starts_with("compadd"))
        .collect();
    assert!(!compadds.is_empty());
    for l in compadds {
        assert!(l.contains("-a values"), "{l}");
        assert!(!l.contains("-Q"), "{l}");
    }
    assert!(!script.contains("eval "), "zsh script must not eval values");
}

/// Runs the real `_toolr` under `zsh -f` with `_files` and `compadd` stubbed
/// to record their arguments; `words` completes the last word.
fn zsh_complete(fx: &Fixture, words: &[&str]) -> Vec<String> {
    const DRIVER: &str = r#"
compdef() { :; }
_files() { print -r -- "FILES:$*"; }
compadd() { print -r -- "COMPADD:$*"; }
eval "$1"; shift
words=("$@"); CURRENT=$#
_toolr
"#;
    let out = StdCommand::new("zsh")
        .arg("-f")
        .arg("-c")
        .arg(DRIVER)
        .arg("driver")
        .arg(print_script("zsh"))
        .args(words)
        .current_dir(fx.tmp.path())
        .env("PATH", path_with_toolr())
        .env("XDG_CACHE_HOME", common::test_cache_home())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

/// zsh's own `_files` quotes the names it inserts.
#[test]
fn zsh_path_slots_use_native_file_completion() {
    if !have("zsh") {
        return;
    }
    let fx = fixture();
    let calls = zsh_complete(&fx, &["toolr", "ci", "lint", "--cfg", "files/"]);
    assert_eq!(calls, vec!["FILES:".to_string()]);
    let calls = zsh_complete(&fx, &["toolr", "ci", "lint", "--out", "files/"]);
    assert_eq!(calls, vec!["FILES:-/".to_string()]);
    let calls = zsh_complete(&fx, &["toolr", "ci", "lint", "--mode", "fa"]);
    assert_eq!(calls, vec!["COMPADD:-d descriptions -a values".to_string()]);
}
