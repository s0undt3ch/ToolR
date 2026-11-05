//! Completion scripts must treat repo-derived candidate values as data.
//! A `Literal[...]` value such as `$(touch X)` must never execute on TAB.
#![cfg(unix)]

mod common;

use std::ffi::CStr;
use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// Files under the fixture root that path slots complete. `files/sub/` is a directory.
const PATH_FILES: [&str; 6] = [
    "files/with space.txt",
    "files/$(touch sentinel-three)",
    "files/back\\slash",
    "files/host:port.txt",
    "files/a:b c.txt",
    "files/v@2x.png",
];

/// `SENTINEL` and `SENTINEL2` stand for paths the fixture fills in.
const CHOICES: [&str; 10] = [
    "fast",
    "a:b",
    "dry run",
    "dry walk",
    "$(touch SENTINEL)",
    "`touch SENTINEL2`",
    "~root",
    "x=~root",
    "u@h",
    "a$b",
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

/// How `COMP_WORDS` holds a word with a `:` or `=`: bash 4+ splits it at
/// `COMP_WORDBREAKS`, unless the user took those characters out of it.
#[derive(Clone, Copy, Debug)]
enum Split {
    Whole,
    AtBreaks,
}

const SPLITS: [Split; 2] = [Split::Whole, Split::AtBreaks];

/// Bash 5's default `COMP_WORDBREAKS`: the driver sets it and [`comp_pieces`] splits at it.
const WORDBREAKS: &str = " \t\n\"'@><=;|&(:";

/// `a:b` becomes `a`, `:`, `b`, and a run of breaks stays one piece, as bash 5 splits them.
/// Blanks and quotes in `breaks` don't split: test words hold none outside an escape or quote.
/// The pty end-to-end test is the oracle for how real readline splits.
fn comp_pieces(word: &str, split: Split, breaks: &str) -> Vec<String> {
    let is_break = |c: char| breaks.contains(c) && !c.is_whitespace() && !matches!(c, '\'' | '"');
    if matches!(split, Split::Whole) || word.is_empty() {
        return vec![word.to_string()];
    }
    let mut pieces: Vec<String> = Vec::new();
    let mut last = None;
    for c in word.chars() {
        if last == Some(is_break(c)) {
            pieces.last_mut().unwrap().push(c);
        } else {
            pieces.push(c.to_string());
        }
        last = Some(is_break(c));
    }
    pieces
}

/// One completion request: `before` are the words between `toolr ci lint` and the
/// cursor's word `cur`, and `tail` the words typed after it.
struct Drive<'a> {
    script: String,
    before: Vec<&'a str>,
    cur: &'a str,
    tail: Vec<&'a str>,
    quote: Vec<String>,
    split: Split,
    /// The break characters [`comp_pieces`] splits at; the script must set the same.
    breaks: String,
    /// Define bash-completion's `_init_completion`, which only the old script calls: a new
    /// binary must stay harmless to an old installed script, so don't delete this as dead.
    legacy: bool,
}

impl<'a> Drive<'a> {
    fn new(script: &str, before: &[&'a str], cur: &'a str) -> Self {
        Drive {
            script: script.to_string(),
            before: before.to_vec(),
            cur,
            tail: Vec::new(),
            quote: Vec::new(),
            split: Split::Whole,
            breaks: WORDBREAKS.to_string(),
            legacy: false,
        }
    }

    fn split(mut self, split: Split) -> Self {
        self.split = split;
        self
    }

    fn breaks(mut self, breaks: &str) -> Self {
        self.breaks = breaks.to_string();
        self
    }

    fn tail(mut self, tail: &[&'a str]) -> Self {
        self.tail = tail.to_vec();
        self
    }

    fn quote(mut self, quote: Vec<String>) -> Self {
        self.quote = quote;
        self
    }

    fn legacy(mut self) -> Self {
        self.legacy = true;
        self
    }

    /// Sets `COMP_LINE`, `COMP_POINT`, `COMP_WORDS` and `COMP_CWORD` as bash does, then
    /// calls `_toolr_complete`. Values reach bash only as positional parameters.
    fn run(&self, bash: &str, fx: &Fixture) -> BashRun {
        const DRIVER: &str = r#"
script=$1 legacy=$2 COMP_LINE=$3 COMP_POINT=$4 COMP_CWORD=$5 k=$6
shift 6
COMP_WORDS=("${@:1:k}")
shift "$k"
# Bash only sets this in an interactive shell; this is readline's default.
COMP_WORDBREAKS=$WORDBREAKS
# Only the frozen old script calls this; it keeps `OLD_BASH_SCRIPT` runnable.
if [ "$legacy" = 1 ]; then
    _init_completion() { words=("${COMP_WORDS[@]}"); cword=$COMP_CWORD; cur=${words[cword]}; prev=${words[cword-1]}; }
fi
# The builtin only works inside a live completion, so record its calls instead.
compopt() { printf 'COMPOPT:%s\n' "$*"; }
eval "$script"
_toolr_complete; rc=$?
printf 'RC:%s\n' "$rc"
for r in "${COMPREPLY[@]}"; do printf 'R:%s\n' "$r"; done
for v in "$@"; do printf 'Q:%s\n' "$(printf '%q' "$v")"; done
"#;
        let head: Vec<&str> = ["toolr", "ci", "lint"]
            .into_iter()
            .chain(self.before.iter().copied())
            .chain([self.cur])
            .collect();
        let line_to_cursor = head.join(" ");
        let line = std::iter::once(line_to_cursor.as_str())
            .chain(self.tail.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        let mut comp_words = Vec::new();
        for w in &head {
            comp_words.extend(comp_pieces(w, self.split, &self.breaks));
        }
        let cword = comp_words.len() - 1;
        for w in &self.tail {
            comp_words.extend(comp_pieces(w, self.split, &self.breaks));
        }
        let out = StdCommand::new(bash)
            .arg("-c")
            .arg(DRIVER)
            .arg("driver")
            .arg(&self.script)
            .arg(if self.legacy { "1" } else { "0" })
            .arg(&line)
            .arg(line_to_cursor.len().to_string())
            .arg(cword.to_string())
            .arg(comp_words.len().to_string())
            .args(&comp_words)
            .args(&self.quote)
            .current_dir(fx.tmp.path())
            .env("WORDBREAKS", WORDBREAKS)
            .env("PATH", path_with_toolr())
            // `~/` completes inside the fixture.
            .env("HOME", fx.tmp.path())
            // The shell runs toolr itself, so mirror `common::toolr`'s cache isolation.
            .env("XDG_CACHE_HOME", common::test_cache_home())
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
}

/// Complete `cur` as the value of `--mode`.
fn run_bash(bash: &str, fx: &Fixture, cur: &str) -> BashRun {
    Drive::new(&print_script("bash"), &["--mode"], cur)
        .quote(fx.choices())
        .run(bash, fx)
}

/// Complete `cur` as the value of a path flag; [`BashRun::quoted`] holds the
/// `%q` forms of [`PATH_FILES`].
fn run_bash_path_split(
    bash: &str,
    script: &str,
    fx: &Fixture,
    flag: &str,
    cur: &str,
    split: Split,
) -> BashRun {
    let quote: Vec<String> = PATH_FILES.iter().map(|f| (*f).to_string()).collect();
    Drive::new(script, &[flag], cur)
        .quote(quote)
        .split(split)
        .run(bash, fx)
}

/// `BASH_VERSINFO` as `major * 100 + minor`, so 4.4 is 404.
fn bash_version(bash: &str) -> Option<u32> {
    let out = StdCommand::new(bash)
        .args(["-c", "echo $((BASH_VERSINFO[0] * 100 + BASH_VERSINFO[1]))"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Every distinct bash 4.4+ on this machine. The script refuses older ones, so on
/// CI having none fails the tests instead of skipping them.
fn bashes() -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(out) = StdCommand::new("sh")
        .args(["-c", "command -v bash"])
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .output()
    {
        candidates.push(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    for p in [
        "/opt/homebrew/bin/bash",
        "/usr/local/bin/bash",
        "/bin/bash",
        "/usr/bin/bash",
    ] {
        candidates.push(p.to_string());
    }
    let mut seen = Vec::new();
    let mut v = Vec::new();
    for p in candidates {
        let Ok(real) = std::fs::canonicalize(&p) else {
            continue;
        };
        if seen.contains(&real) {
            continue;
        }
        seen.push(real);
        if bash_version(&p).is_some_and(|ver| ver >= 404) {
            v.push(p);
        }
    }
    assert!(
        !v.is_empty() || std::env::var_os("CI").is_none(),
        "no bash 4.4 or newer found; CI must install one so the bash completion tests run"
    );
    if v.is_empty() {
        eprintln!("skipping: no bash 4.4 or newer found");
    }
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
        // Readline keeps the `x=` typed before a break, so the reply holds only the rest.
        for (cur, want, split) in SPLITS.into_iter().flat_map(|split| {
            [
                ("", "\\~root", split),
                ("~r", "\\~root", split),
                ("\\~r", "\\~root", split),
                ("", "x=\\~root", split),
                ("x=~", "\\~root", split),
                ("x=\\~", "\\~root", split),
            ]
        }) {
            let run = Drive::new(&print_script("bash"), &["--mode"], cur)
                .split(split)
                .run(&bash, &fx);
            assert_eq!(run.rc, "0", "{bash} {cur:?} {split:?}: {}", run.stderr);
            assert!(
                run.reply.contains(&want.to_string()),
                "{bash} {cur:?} {split:?}: {:?}",
                run.reply
            );
            assert!(
                !run.reply.iter().any(|r| r.replace("\\~", "").contains('~')),
                "{bash} {cur:?} {split:?}: bare tilde in {:?}",
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
        // `dry\ ` is how bash inserted the common prefix `dry `.
        let run = run_bash(&bash, &fx, "dry\\ ");
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
        let run = Drive::new(&print_script("bash"), &["--mode"], "fa")
            .tail(&["--verbose"])
            .run(&bash, &fx);
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert_eq!(run.reply, vec!["fast".to_string()], "{bash}");
    }
}

#[test]
fn bash_path_flag_completes_files_quoted_and_inert() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash_path_split(
            &bash,
            &print_script("bash"),
            &fx,
            "--cfg",
            "files/",
            Split::Whole,
        );
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
        let run = run_bash_path_split(
            &bash,
            &print_script("bash"),
            &fx,
            "--out",
            "files/",
            Split::Whole,
        );
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert_eq!(run.reply, vec!["files/sub/".to_string()], "{bash}");
        assert_eq!(run.compopt, vec!["-o nospace".to_string()], "{bash}");
    }
}

#[test]
fn bash_single_directory_match_on_a_path_flag_drops_the_space() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash_path_split(
            &bash,
            &print_script("bash"),
            &fx,
            "--cfg",
            "files/s",
            Split::Whole,
        );
        assert_eq!(run.reply, vec!["files/sub/".to_string()], "{bash}");
        assert_eq!(run.compopt, vec!["-o nospace".to_string()], "{bash}");
        let run = run_bash_path_split(
            &bash,
            &print_script("bash"),
            &fx,
            "--cfg",
            "files/w",
            Split::Whole,
        );
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
        let run = run_bash_path_split(
            &bash,
            &print_script("bash"),
            &fx,
            "--cfg",
            "~/files/",
            Split::Whole,
        );
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

/// Native bash keeps a typed `$HOME/` as typed; escaping it would insert a literal `\$HOME`.
#[test]
fn bash_path_keeps_a_typed_leading_variable() {
    for bash in bashes() {
        for (var, split) in ["$HOME", "${HOME}"]
            .into_iter()
            .flat_map(|v| SPLITS.map(|s| (v, s)))
        {
            let fx = fixture();
            let script = print_script("bash");
            let cur = format!("{var}/files/s");
            let run = run_bash_path_split(&bash, &script, &fx, "--cfg", &cur, split);
            assert_eq!(run.rc, "0", "{bash} {var} {split:?}: {}", run.stderr);
            assert_eq!(run.reply, vec![format!("{var}/files/sub/")], "{bash} {var}");
            assert_eq!(run.compopt, vec!["-o nospace".to_string()], "{bash} {var}");
            let cur = format!("{var}/files/");
            let run = run_bash_path_split(&bash, &script, &fx, "--cfg", &cur, split);
            let mut want: Vec<String> = run.quoted.iter().map(|q| format!("{var}/{q}")).collect();
            want.push(format!("{var}/files/sub/"));
            want.sort();
            let mut got = run.reply.clone();
            got.sort();
            assert_eq!(got, want, "{bash} {var} {split:?}");
            let cur = format!("{var}/files/host:p");
            let run = run_bash_path_split(&bash, &script, &fx, "--cfg", &cur, split);
            assert_eq!(
                run.reply,
                vec!["port.txt".to_string()],
                "{bash} {var} {split:?}"
            );
            let cur = format!("{var}/fil");
            let run = run_bash_path_split(&bash, &script, &fx, "--out", &cur, split);
            assert_eq!(
                run.reply,
                vec![format!("{var}/files/")],
                "{bash} {var} {split:?}"
            );
            assert!(!fx.sentinel3.exists(), "{bash} {var}: file name executed");
        }
    }
}

/// Like native bash, an unset variable lists nothing, and under `set -u` doesn't error.
#[test]
fn bash_path_under_an_unset_variable_offers_nothing() {
    for bash in bashes() {
        let fx = fixture();
        let script = format!("set -eu\n{}", print_script("bash"));
        for cur in ["$TOOLR_TEST_UNSET_VAR/f", "${TOOLR_TEST_UNSET_VAR}/f"] {
            let run = run_bash_path_split(&bash, &script, &fx, "--cfg", cur, Split::Whole);
            assert_eq!(run.rc, "0", "{bash} {cur}: {}", run.stderr);
            assert_eq!(run.stderr, "", "{bash} {cur}");
            assert!(run.reply.is_empty(), "{bash} {cur}: {:?}", run.reply);
        }
    }
}

/// Only a plain `$NAME` or `${NAME}` is expanded: substitutions and operators stay inert.
#[test]
fn bash_path_never_runs_a_typed_substitution() {
    for bash in bashes() {
        let fx = fixture();
        let sentinel = fx.sentinel.to_str().unwrap();
        for cur in [
            format!("$(touch {sentinel})/"),
            format!("${{HOME:-$(touch {sentinel})}}/"),
            format!("`touch {sentinel}`/"),
            "$1/".to_string(),
        ] {
            let run = run_bash_path_split(
                &bash,
                &print_script("bash"),
                &fx,
                "--cfg",
                &cur,
                Split::Whole,
            );
            assert_eq!(run.rc, "0", "{bash} {cur}: {}", run.stderr);
            assert!(run.reply.is_empty(), "{bash} {cur}: {:?}", run.reply);
            assert!(
                !fx.sentinel.exists(),
                "{bash} {cur}: typed substitution ran"
            );
        }
    }
}

/// A user's `set -e` or `set -u` must not leak into Tab: a non-zero
/// `__complete` exit or an unset array would otherwise close their shell.
#[test]
fn bash_completion_survives_errexit() {
    for (bash, opts) in bashes()
        .into_iter()
        .flat_map(|b| [(b.clone(), "-e"), (b, "-eu")])
    {
        let fx = fixture();
        let script = format!("set {opts}\n{}", print_script("bash"));
        let run = run_bash_path_split(&bash, &script, &fx, "--out", "files/", Split::Whole);
        assert_eq!(run.rc, "0", "{bash} {opts}: {}", run.stderr);
        assert_eq!(run.reply, vec!["files/sub/".to_string()], "{bash} {opts}");
        let run = run_bash_path_split(&bash, &script, &fx, "--cfg", "files/s", Split::Whole);
        assert_eq!(run.rc, "0", "{bash} {opts}: {}", run.stderr);
        assert_eq!(run.reply, vec!["files/sub/".to_string()], "{bash} {opts}");
        // No directory matches, so the directory probe itself exits non-zero.
        let run = run_bash_path_split(&bash, &script, &fx, "--cfg", "files/w", Split::Whole);
        assert_eq!(run.rc, "0", "{bash} {opts}: {}", run.stderr);
        assert_eq!(run.reply, vec![run.quoted[0].clone()], "{bash} {opts}");
        let run = run_bash_path_split(&bash, &script, &fx, "--cfg", "nomatch", Split::Whole);
        assert_eq!(run.rc, "0", "{bash} {opts}: {}", run.stderr);
        assert!(run.reply.is_empty(), "{bash} {opts}: {:?}", run.reply);
        let run = Drive::new(&script, &["--mode"], "fa").run(&bash, &fx);
        assert_eq!(run.rc, "0", "{bash} {opts}: {}", run.stderr);
        assert_eq!(run.reply, vec!["fast".to_string()], "{bash} {opts}");
        let run = Drive::new(&script, &["--mode"], "zz").run(&bash, &fx);
        assert_eq!(run.rc, "0", "{bash} {opts}: {}", run.stderr);
        assert!(run.reply.is_empty(), "{bash} {opts}: {:?}", run.reply);
        for split in SPLITS {
            let run = run_bash_path_split(&bash, &script, &fx, "--cfg", "files/host:p", split);
            assert_eq!(run.rc, "0", "{bash} {opts} {split:?}: {}", run.stderr);
            assert_eq!(
                run.reply,
                vec!["port.txt".to_string()],
                "{bash} {opts} {split:?}"
            );
            let run = Drive::new(&script, &["--mode"], "a:")
                .split(split)
                .run(&bash, &fx);
            assert_eq!(run.rc, "0", "{bash} {opts} {split:?}: {}", run.stderr);
            assert_eq!(run.reply, vec!["b".to_string()], "{bash} {opts} {split:?}");
        }
    }
}

/// The script must not need bash-completion: nothing defines `_init_completion` here.
#[test]
fn bash_completion_runs_without_bash_completion() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash(&bash, &fx, "fa");
        assert_eq!(run.stderr, "", "{bash}");
        assert_eq!(run.reply, vec!["fast".to_string()], "{bash}");
    }
}

/// Readline replaces only the text after the last `:` or `=`, so a reply that
/// repeated the typed prefix would insert it twice.
#[test]
fn bash_colon_in_a_file_name_completes_after_the_colon() {
    for bash in bashes() {
        for split in SPLITS {
            let fx = fixture();
            let script = print_script("bash");
            let run = run_bash_path_split(&bash, &script, &fx, "--cfg", "files/host:p", split);
            assert_eq!(run.rc, "0", "{bash} {split:?}: {}", run.stderr);
            assert_eq!(run.stderr, "", "{bash} {split:?}");
            assert_eq!(run.reply, vec!["port.txt".to_string()], "{bash} {split:?}");
            // Only the part after the colon is quoted.
            let run = run_bash_path_split(&bash, &script, &fx, "--cfg", "files/a:b", split);
            assert_eq!(run.reply, vec!["b\\ c.txt".to_string()], "{bash} {split:?}");
            assert!(
                !fx.sentinel3.exists(),
                "{bash} {split:?}: file name executed"
            );
        }
    }
}

#[test]
fn bash_colon_after_a_typed_tilde_marks_the_directory() {
    for bash in bashes() {
        for split in SPLITS {
            let fx = fixture();
            std::fs::create_dir_all(fx.tmp.path().join("files/d:ir")).unwrap();
            let script = print_script("bash");
            let run = run_bash_path_split(&bash, &script, &fx, "--cfg", "~/files/d:", split);
            assert_eq!(run.rc, "0", "{bash} {split:?}: {}", run.stderr);
            assert_eq!(run.reply, vec!["ir/".to_string()], "{bash} {split:?}");
            assert_eq!(
                run.compopt,
                vec!["-o nospace".to_string()],
                "{bash} {split:?}"
            );
            let run = run_bash_path_split(&bash, &script, &fx, "--cfg", "~/files/host:p", split);
            assert_eq!(run.reply, vec!["port.txt".to_string()], "{bash} {split:?}");
        }
    }
}

#[test]
fn bash_colon_in_a_choice_completes_after_the_colon() {
    for bash in bashes() {
        for split in SPLITS {
            let fx = fixture();
            let run = Drive::new(&print_script("bash"), &["--mode"], "a:")
                .split(split)
                .run(&bash, &fx);
            assert_eq!(run.rc, "0", "{bash} {split:?}: {}", run.stderr);
            assert_eq!(run.reply, vec!["b".to_string()], "{bash} {split:?}");
        }
    }
}

/// Readline replaces text from a `@` or `$` break, bash's special prefixes, not after it,
/// so the reply keeps the typed `@` or `$`.
#[test]
fn bash_special_prefix_break_stays_in_the_reply() {
    for bash in bashes() {
        for split in SPLITS {
            let fx = fixture();
            let script = print_script("bash");
            let run = Drive::new(&script, &["--mode"], "u@")
                .split(split)
                .run(&bash, &fx);
            assert_eq!(run.rc, "0", "{bash} {split:?}: {}", run.stderr);
            assert_eq!(run.reply, vec!["@h".to_string()], "{bash} {split:?}");
            let run = run_bash_path_split(&bash, &script, &fx, "--cfg", "files/v@", split);
            assert_eq!(run.rc, "0", "{bash} {split:?}: {}", run.stderr);
            assert_eq!(run.reply, vec!["@2x.png".to_string()], "{bash} {split:?}");
            // `$` isn't a default break, so only a user who adds it gets the split.
            let run = Drive::new(&script, &["--mode"], "a$")
                .split(split)
                .run(&bash, &fx);
            assert_eq!(run.reply, vec!["a\\$b".to_string()], "{bash} {split:?}");
            let dollar = format!("COMP_WORDBREAKS+='$'\n{script}");
            let run = Drive::new(&dollar, &["--mode"], "a$")
                .split(split)
                .breaks(&format!("{WORDBREAKS}$"))
                .run(&bash, &fx);
            assert_eq!(run.rc, "0", "{bash} {split:?}: {}", run.stderr);
            assert_eq!(run.reply, vec!["\\$b".to_string()], "{bash} {split:?}");
        }
    }
}

/// Bash 4+ splits an earlier `files/host:port.txt` into three `COMP_WORDS`; the
/// engine must still see one word, or `--mode` is parsed as a stray value.
#[test]
fn bash_earlier_word_with_a_colon_stays_one_word() {
    for bash in bashes() {
        for split in SPLITS {
            let fx = fixture();
            let run = Drive::new(
                &print_script("bash"),
                &["--cfg", "files/host:port.txt", "--mode"],
                "fa",
            )
            .split(split)
            .run(&bash, &fx);
            assert_eq!(run.rc, "0", "{bash} {split:?}: {}", run.stderr);
            assert_eq!(run.reply, vec!["fast".to_string()], "{bash} {split:?}");
        }
    }
}

/// Marking directories must not rescan the whole listing for every name. Wall-clock
/// bounds flake under load, so compare 16x the entries against 1x in the same run. 64x is the
/// pass threshold: linear work grows about 16x, a per-name rescan about 256x. The wide span keeps
/// the fixed cost of starting bash and toolr small next to the listing.
#[test]
fn bash_path_completion_scales_linearly_with_the_listing() {
    const SMALL: usize = 1000;
    const LARGE: usize = 16 * SMALL;
    for bash in bashes() {
        let fx = fixture();
        let script = print_script("bash");
        for (name, n) in [("small", SMALL), ("large", LARGE)] {
            let dir = fx.tmp.path().join(name);
            std::fs::create_dir_all(&dir).unwrap();
            for i in 0..n {
                std::fs::create_dir_all(dir.join(format!("d{i:05}"))).unwrap();
                std::fs::write(dir.join(format!("f{i:05}")), "").unwrap();
            }
        }
        let time = |cur: &str, n: usize| {
            let start = Instant::now();
            let run = Drive::new(&script, &["--cfg"], cur).run(&bash, &fx);
            let took = start.elapsed();
            assert_eq!(run.rc, "0", "{bash} {cur}: {}", run.stderr);
            assert_eq!(run.reply.len(), 2 * n, "{bash} {cur}");
            let dirs = run.reply.iter().filter(|r| r.ends_with('/')).count();
            assert_eq!(dirs, n, "{bash} {cur}");
            took
        };
        // `~/` takes the path that looks directories up in `compgen -d`'s output.
        for prefix in ["", "~/"] {
            // The fastest of three, with the sizes interleaved, so a burst of load on the
            // runner doesn't land on every run of one size.
            let (mut small, mut large) = (Duration::MAX, Duration::MAX);
            for _ in 0..3 {
                small = small.min(time(&format!("{prefix}small/"), SMALL));
                large = large.min(time(&format!("{prefix}large/"), LARGE));
            }
            let ratio = large.as_secs_f64() / small.as_secs_f64();
            assert!(
                ratio < 64.0,
                "{bash} {prefix:?}: 16x the entries took {ratio:.1}x as long ({small:?} -> {large:?})"
            );
        }
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
    // `~user` ignores `$HOME`, so read (never write) a directory already in the real home whose
    // name is a prefix of nothing else there, so the reply is exactly that one directory.
    let (user, home) = passwd_entry();
    let names: Vec<String> = std::fs::read_dir(&home)
        .map(|rd| {
            rd.filter_map(|e| e.ok()?.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    let plain = |n: &str| {
        n.chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    };
    let Some(dir) = names.iter().find(|n| {
        !n.starts_with('.')
            && plain(n)
            && home.join(n).is_dir()
            && names.iter().filter(|m| m.starts_with(n.as_str())).count() == 1
    }) else {
        assert!(
            std::env::var_os("CI").is_none(),
            "no uniquely named plain directory in {} to complete `~{user}/` against",
            home.display()
        );
        eprintln!(
            "skipping: no uniquely named directory in {}",
            home.display()
        );
        return;
    };
    let cur = format!("~{user}/{dir}");
    let want = format!("~{user}/{dir}/");
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash_path_split(
            &bash,
            &print_script("bash"),
            &fx,
            "--cfg",
            &cur,
            Split::Whole,
        );
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert_eq!(run.reply, vec![want.clone()], "{bash}");
        assert_eq!(run.compopt, vec!["-o nospace".to_string()], "{bash}");
    }
}

/// A typed `\\` stands for one literal backslash in the name.
#[test]
fn bash_path_unescapes_a_typed_backslash() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash_path_split(
            &bash,
            &print_script("bash"),
            &fx,
            "--cfg",
            "files/back\\\\s",
            Split::Whole,
        );
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert_eq!(run.reply, vec![run.quoted[2].clone()], "{bash}");
        let run = run_bash_path_split(
            &bash,
            &print_script("bash"),
            &fx,
            "--cfg",
            "files/with\\ s",
            Split::Whole,
        );
        assert_eq!(run.reply, vec![run.quoted[0].clone()], "{bash}");
    }
}

/// The bash script shipped before path completion, as an installed copy would hold it.
/// Frozen on purpose: the next test checks a new binary is harmless to an old installed script.
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

/// A new binary must stay harmless to an old installed script (frozen as `OLD_BASH_SCRIPT`).
#[test]
fn old_bash_script_offers_nothing_on_a_path_slot() {
    for bash in bashes() {
        let fx = fixture();
        let run = Drive::new(OLD_BASH_SCRIPT, &["--cfg"], "files/")
            .legacy()
            .run(&bash, &fx);
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert_eq!(run.stderr, "", "{bash}");
        assert!(run.reply.is_empty(), "{bash}: {:?}", run.reply);
    }
}

/// An older bash registers no completion it can't run, and an interactive one warns once
/// when sourcing; a non-interactive one, such as a bashrc sourced under `bash -c`, is silent.
#[test]
fn bash_older_than_4_4_warns_once_when_interactive_and_registers_nothing() {
    let old = "/bin/bash";
    if !bash_version(old).is_some_and(|v| v < 404) {
        eprintln!("skipping: no bash older than 4.4 at {old}");
        return;
    }
    let fx = fixture();
    let path = fx.tmp.path().join("toolr.bash");
    std::fs::write(&path, print_script("bash")).unwrap();
    let out = StdCommand::new(old)
        .arg("-c")
        .arg(r#"set -eu; source "$1"; echo SOURCED; complete -p toolr 2>/dev/null && echo REGISTERED || :"#)
        .arg("driver")
        .arg(&path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert_eq!(stdout, "SOURCED\n");
    assert_eq!(stderr, "");

    let rcfile = fx.tmp.path().join("bashrc");
    std::fs::write(&rcfile, format!("set -u\nsource '{}'\n", path.display())).unwrap();
    let mut cmd = StdCommand::new(old);
    cmd.args(["--noprofile", "--rcfile"])
        .arg(&rcfile)
        .arg("-i")
        .env("TERM", "dumb")
        .env("INPUTRC", "/dev/null");
    // Readline echoes the typed line, so the marker printed differs from the text typed.
    let out = common::run_on_pty(
        cmd,
        "complete -p toolr >/dev/null 2>&1 || echo NOT-$?\nexit\n",
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stdout.lines().any(|l| l.trim_end_matches('\r') == "NOT-1"),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    let warning = "toolr: tab completion needs bash >= 4.4 (this is ";
    assert_eq!(stderr.matches(warning).count(), 1, "{stderr}");
    let line = stderr.lines().find(|l| l.contains(warning)).unwrap();
    assert!(
        line.trim_end_matches('\r')
            .ends_with("); upgrade bash to enable it."),
        "{stderr}"
    );
}

/// The one test where readline itself splits the line and inserts the reply: an
/// interactive bash on a pty, with Tab and Enter typed ahead.
#[test]
fn bash_tab_completes_words_with_a_colon_end_to_end() {
    for bash in bashes() {
        let fx = fixture();
        let script = fx.tmp.path().join("toolr.bash");
        std::fs::write(&script, print_script("bash")).unwrap();
        let rc = format!(
            "source '{}'\n\
             toolr() {{ if [[ ${{1-}} == __complete ]]; then command toolr \"$@\"; \
             else printf 'LINE:%s\\n' \"$*\"; fi; }}\n",
            script.display()
        );
        let rcfile = fx.tmp.path().join("bashrc");
        std::fs::write(&rcfile, &rc).unwrap();
        // `$` isn't a default break, but a user may add it. The whole value, since `+=` in a
        // bashrc appends to an empty one: readline only sets the default later.
        let dollar_rcfile = fx.tmp.path().join("bashrc-dollar");
        std::fs::write(
            &dollar_rcfile,
            format!("{rc}COMP_WORDBREAKS=$' \\t\\n\"\\'@><=;|&(:$'\n"),
        )
        .unwrap();
        for (rcfile, typed, want) in [
            (
                &rcfile,
                "toolr ci lint --cfg files/host:p",
                "LINE:ci lint --cfg files/host:port.txt",
            ),
            (
                &rcfile,
                "toolr ci lint --mode a:",
                "LINE:ci lint --mode a:b",
            ),
            // Readline replaces from a `@` or `$` break, so the reply must keep it.
            (
                &rcfile,
                "toolr ci lint --mode u@",
                "LINE:ci lint --mode u@h",
            ),
            (
                &rcfile,
                "toolr ci lint --cfg files/v@",
                "LINE:ci lint --cfg files/v@2x.png",
            ),
            (
                &dollar_rcfile,
                "toolr ci lint --mode a$",
                "LINE:ci lint --mode a$b",
            ),
        ] {
            let mut cmd = StdCommand::new(&bash);
            cmd.args(["--noprofile", "--rcfile"])
                .arg(rcfile)
                .arg("-i")
                .current_dir(fx.tmp.path())
                .env("PATH", path_with_toolr())
                .env("HOME", fx.tmp.path())
                .env("XDG_CACHE_HOME", common::test_cache_home())
                // No escape sequences, and no user readline settings.
                .env("TERM", "dumb")
                .env("INPUTRC", "/dev/null");
            let out = common::run_on_pty(cmd, &format!("{typed}\t\nexit\n"));
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                stdout.lines().any(|l| l.trim_end_matches('\r') == want),
                "{bash} {typed:?}: want {want:?}\nstdout: {stdout}\nstderr: {stderr}"
            );
            assert!(!stderr.contains("command not found"), "{bash}: {stderr}");
        }
    }
}

fn have(shell: &str) -> bool {
    StdCommand::new(shell)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Whether `shell` can run; on CI a missing shell fails its tests instead of skipping them.
fn require(shell: &str) -> bool {
    let found = have(shell);
    assert!(
        found || std::env::var_os("CI").is_none(),
        "{shell} is not installed; CI must install it so its completion tests run"
    );
    found
}

#[test]
fn fish_completion_never_executes_values() {
    if !require("fish") {
        return;
    }
    let fx = fixture();
    let stdout = fish_complete(&fx, "toolr ci lint --mode ");
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
    if !require("fish") {
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
    if !require("zsh") {
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
