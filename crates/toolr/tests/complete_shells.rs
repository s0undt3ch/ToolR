//! Completion scripts must treat repo-derived candidate values as data.
//! A `Literal[...]` value such as `$(touch X)` must never execute on TAB.
#![cfg(unix)]

mod common;

use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// Files under the fixture root that path slots complete. `files/sub/` is a directory.
const PATH_FILES: [&str; 8] = [
    "files/with space.txt",
    "files/$(touch sentinel-three)",
    "files/back\\slash",
    "files/host:port.txt",
    "files/a:b c.txt",
    "files/v@2x.png",
    "files/it's.txt",
    "files/q\"d.txt",
];

/// `SENTINEL` and `SENTINEL2` stand for paths the fixture fills in.
const CHOICES: [&str; 12] = [
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
    "wow!",
    // `"\!"…` starts with the `"` readline won't double.
    "!;touch SENTINEL;#",
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
    verbose: bool = False,
):
    """Lint things.

    Args:
        mode: Lint mode.
        cfg: Config path.
        out: Output directory.
        verbose: Say more.
    """
    pass

@group.command
def pos(ctx, target: Literal["alpha", "beta"], flavour: str = "plain"):
    """A positional and a flag.

    Args:
        target: What to act on.
        flavour: Which kind.
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
    /// The reply in sorted order, for replies whose order doesn't matter.
    fn sorted_reply(&self) -> Vec<String> {
        let mut reply = self.reply.clone();
        reply.sort();
        reply
    }

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
            // `current_dir` leaves the parent's `PWD`. A stale one makes bash call `getcwd`, which
            // fails on a conda bash 5.1 for macOS and leaves `$PWD` empty.
            .env("PWD", fx.tmp.path())
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

fn version_key(dir: &std::path::Path) -> Vec<u32> {
    dir.file_name()
        .map(|n| {
            n.to_string_lossy()
                .split('.')
                .filter_map(|c| c.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

const TEST_BASH_VAR: &str = "TOOLR_TEST_BASH";
const SKIP_BASH_VAR: &str = "TOOLR_SKIP_BASH_COMPLETION_TESTS";

/// `bin/bash` of the newest mise-installed `conda:bash`, which is where a contributor on a stock macOS
/// checkout gets a bash 4.4+ from (`mise use -g conda:bash@5.3`), plus the PATH-style list in `TOOLR_TEST_BASH`.
fn extra_bash_candidates() -> Vec<String> {
    let mut found: Vec<String> = std::env::var_os(TEST_BASH_VAR)
        .map(|v| {
            std::env::split_paths(&v)
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| p.to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let data = std::env::var_os("MISE_DATA_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share/mise"))
        });
    if let Some(dir) = data.map(|d| d.join("installs/conda-bash"))
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        // Newest only: every test loops over `bashes()`, so each extra install multiplies the
        // suite's run time. `TOOLR_TEST_BASH` targets a specific older one.
        let mut versions: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("bin/bash").is_file())
            .collect();
        versions.sort_by_key(|p| version_key(p));
        found.extend(
            versions
                .last()
                .map(|p| p.join("bin/bash").to_string_lossy().into_owned()),
        );
    }
    found
}

/// Every distinct bash 4.4+ on this machine. The script refuses older ones. With none, CI fails
/// the tests; elsewhere they pass vacuously, which `a_bash_4_4_or_newer_was_found` turns red.
fn bashes() -> Vec<String> {
    // Probe once: ~70 tests call this and each probe spawns bash.
    static FOUND: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    FOUND.get_or_init(find_bashes).clone()
}

fn find_bashes() -> Vec<String> {
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
    candidates.extend(extra_bash_candidates());
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
    v
}

#[test]
fn a_bash_4_4_or_newer_was_found() {
    assert!(
        !bashes().is_empty() || std::env::var_os(SKIP_BASH_VAR).is_some_and(|v| v == "1"),
        "no bash 4.4+ found, so every bash completion test was a no-op. Install one \
         (`mise use -g conda:bash@5.3`, or `brew install bash`), point TOOLR_TEST_BASH at one, or \
         set TOOLR_SKIP_BASH_COMPLETION_TESTS=1 to accept skipping them (local runs only; CI \
         always fails without a bash)"
    );
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
        let got = run.sorted_reply();
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
            // A single matching directory completes to it, with a trailing slash.
            let cur = format!("{var}/files/s");
            let run = run_bash_path_split(&bash, &script, &fx, "--cfg", &cur, split);
            assert_eq!(run.rc, "0", "{bash} {var} {split:?}: {}", run.stderr);
            assert_eq!(
                run.reply,
                vec![format!("{var}/files/sub/")],
                "{bash} {var} {split:?}"
            );
            assert_eq!(
                run.compopt,
                vec!["-o nospace".to_string()],
                "{bash} {var} {split:?}"
            );
            // A full listing keeps the typed variable on every entry.
            let cur = format!("{var}/files/");
            let run = run_bash_path_split(&bash, &script, &fx, "--cfg", &cur, split);
            let mut want: Vec<String> = run.quoted.iter().map(|q| format!("{var}/{q}")).collect();
            want.push(format!("{var}/files/sub/"));
            want.sort();
            let got = run.sorted_reply();
            assert_eq!(got, want, "{bash} {var} {split:?}");
            // `:` is a word break: readline replaces only the text after it, so the reply is bare.
            let cur = format!("{var}/files/host:p");
            let run = run_bash_path_split(&bash, &script, &fx, "--cfg", &cur, split);
            assert_eq!(
                run.reply,
                vec!["port.txt".to_string()],
                "{bash} {var} {split:?}"
            );
            // A directory-only slot lists directories alone.
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

/// `${!name}` sees the script's own locals, so a user variable sharing a former local's name
/// must still read the user's value.
#[test]
fn bash_path_variable_named_like_a_script_local_reads_the_users_value() {
    for bash in bashes() {
        let fx = fixture();
        for name in ["value", "name", "cur", "var", "mode", "raw_cur", "breaks"] {
            let script = format!("{name}=$PWD\n{}", print_script("bash"));
            let cur = format!("${name}/fi");
            let run = run_bash_path_split(&bash, &script, &fx, "--cfg", &cur, Split::Whole);
            assert_eq!(run.rc, "0", "{bash} {name}: {}", run.stderr);
            assert_eq!(run.reply, vec![format!("${name}/files/")], "{bash} {name}");
        }
    }
}

/// Bash doesn't tilde-expand an expanded value, so a variable holding `~`, `~u` or a relative
/// path names a literal directory under the working directory.
#[test]
fn bash_path_variable_value_is_a_literal_directory() {
    for bash in bashes() {
        for (value, dir) in [("~", "XX"), ("~u", "XX"), ("rel", "R")] {
            let fx = fixture();
            let literal = fx.tmp.path().join(value);
            std::fs::create_dir_all(literal.join("sub")).unwrap();
            std::fs::write(literal.join("lit.txt"), "").unwrap();
            let script = format!("{dir}='{value}'\n{}", print_script("bash"));
            let cur = format!("${dir}/");
            let run = run_bash_path_split(&bash, &script, &fx, "--cfg", &cur, Split::Whole);
            assert_eq!(run.rc, "0", "{bash} {value}: {}", run.stderr);
            let got = run.sorted_reply();
            assert_eq!(
                got,
                vec![format!("${dir}/lit.txt"), format!("${dir}/sub/")],
                "{bash} {value}"
            );
            if value == "~" {
                let run = run_bash_path_split(&bash, &script, &fx, "--out", &cur, Split::Whole);
                assert_eq!(run.reply, vec![format!("${dir}/sub/")], "{bash}");
            }
        }
    }
}

/// `${!name}` reads through dynamic scope, so every name the script declares must be
/// `_toolr_`-prefixed or it can shadow the user's variable.
#[test]
fn bash_script_names_its_variables_with_the_toolr_prefix() {
    let script = print_script("bash");
    let mut names: Vec<(String, &str)> = Vec::new();
    for line in script
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
    {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.first() == Some(&"local") {
            for w in &words[1..] {
                if w.starts_with('-') {
                    continue;
                }
                let name = w.split('=').next().unwrap();
                // Skips the tail of a value that contains a space, e.g. `${a[n - 1]}`.
                if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    names.push((name.to_string(), line));
                }
            }
        }
        if words.first() == Some(&"for") {
            let target = words[1].trim_start_matches("((");
            let target = if target.is_empty() { words[2] } else { target };
            names.push((target.to_string(), line));
        }
        if let Some(at) = words.iter().position(|w| *w == "read") {
            let mut skip = false;
            for w in &words[at + 1..] {
                // Only `-d` takes an argument in this script, and `-d ''`'s is not a name.
                if std::mem::take(&mut skip) {
                    continue;
                }
                if w.starts_with('-') {
                    skip = *w == "-d";
                    continue;
                }
                let name = w.trim_end_matches(';');
                if name != "_" {
                    names.push((name.to_string(), line));
                }
                break;
            }
        }
    }
    assert!(names.len() > 20, "parsed too few declarations: {names:?}");
    for (name, line) in &names {
        assert!(name.starts_with("_toolr_"), "`{name}` in `{line}`");
    }
}

/// Creates `name` under the fixture root as a literal directory holding a file, so a word
/// handed to a `compgen` that doesn't expand it (bash < 5.3) still lists something.
fn literal_dir(fx: &Fixture, name: &str) {
    let dir = fx.tmp.path().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("inside"), "").unwrap();
}

/// Inside the completion function `$_` is its own last argument, not the user's, so `$_/` gets no
/// variable treatment. Bash 5.3 `compgen` would expand it, and older bash would list a directory
/// literally named `$_`; either way the word must offer nothing.
#[test]
fn bash_path_underscore_is_not_treated_as_a_variable() {
    for bash in bashes() {
        let fx = fixture();
        let script = print_script("bash");
        for cur in ["$_/", "${_}/"] {
            literal_dir(&fx, cur.trim_end_matches('/'));
            let run = run_bash_path_split(&bash, &script, &fx, "--cfg", cur, Split::Whole);
            assert_eq!(run.rc, "0", "{bash} {cur}: {}", run.stderr);
            assert!(run.reply.is_empty(), "{bash} {cur}: {:?}", run.reply);
        }
    }
}

/// Only a plain `$NAME` or `${NAME}` is expanded. Bash 5.3 `compgen` runs a `$(...)` or backtick
/// inside `${...}` whose default is taken, so any other `$` or backtick form offers nothing.
/// Each word also names a literal directory, so older bash, which doesn't expand, would list it.
#[test]
fn bash_path_never_runs_a_typed_substitution() {
    const SUB: &str = "$(touch sentinel-one)";
    const TICK: &str = "`touch sentinel-one`";
    let unset_default = format!("${{TOOLR_TEST_UNSET_VAR:-{SUB}}}");
    let home_default = format!("${{HOME:-{SUB}}}");
    let script = print_script("bash");
    let unset_home = format!("unset HOME\n{script}");
    // The value of a plain `$NAME` is read, never expanded again by `compgen`.
    let evil_value = format!("TOOLR_TEST_EVIL='{unset_default}'\n{script}");
    // (script, literal directory, typed word)
    let cases: Vec<(&str, String, String)> = vec![
        (&script, SUB.to_string(), format!("{SUB}/")),
        (&script, TICK.to_string(), format!("{TICK}/")),
        (&script, unset_default.clone(), format!("{unset_default}/")),
        (
            &script,
            format!("${{TOOLR_TEST_UNSET_VAR:-{TICK}}}"),
            format!("${{TOOLR_TEST_UNSET_VAR:-{TICK}}}/"),
        ),
        (&script, home_default.clone(), format!("{home_default}/")),
        (
            &unset_home,
            home_default.clone(),
            format!("{home_default}/"),
        ),
        (&unset_home, SUB.to_string(), format!("{SUB}/")),
        (
            &script,
            format!("files/{unset_default}"),
            format!("files/{unset_default}/"),
        ),
        (
            &script,
            unset_default.clone(),
            format!("$HOME/{unset_default}/"),
        ),
        (
            &evil_value,
            unset_default.clone(),
            "$TOOLR_TEST_EVIL/".to_string(),
        ),
        (&script, "$1".to_string(), "$1/".to_string()),
    ];
    for bash in bashes() {
        let fx = fixture();
        for (script, dir, cur) in &cases {
            literal_dir(&fx, dir);
            let run = run_bash_path_split(&bash, script, &fx, "--cfg", cur, Split::Whole);
            let home = if script.starts_with("unset HOME") {
                " (HOME unset)"
            } else {
                ""
            };
            // Checked first: a command that ran is the finding, whatever the reply.
            assert!(
                !fx.sentinel.exists(),
                "{bash} {cur}{home}: typed substitution ran; reply {:?}",
                run.reply
            );
            assert_eq!(run.rc, "0", "{bash} {cur}{home}: {}", run.stderr);
            assert!(run.reply.is_empty(), "{bash} {cur}{home}: {:?}", run.reply);
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
        for (cur, want) in [
            ("--mode=fa", &["fast"][..]),
            ("--mode=zz", &[]),
            ("--cfg=files/s", &["files/sub/"]),
            ("--out=nomatch", &[]),
            ("--verbose=", &[]),
        ] {
            let run = Drive::new(&script, &[], cur).run(&bash, &fx);
            assert_eq!(run.rc, "0", "{bash} {opts} {cur}: {}", run.stderr);
            assert_eq!(run.stderr, "", "{bash} {opts} {cur}");
            assert_eq!(run.reply, want, "{bash} {opts} {cur}");
        }
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

/// Completes the path `cur` after `--cfg` with the whole word in `COMP_WORDS`.
fn run_bash_cfg_path(bash: &str, fx: &Fixture, cur: &str) -> BashRun {
    run_bash_path_split(bash, &print_script("bash"), fx, "--cfg", cur, Split::Whole)
}

/// Asserts completing the path `cur` after `--cfg` offers exactly `want`.
fn assert_path_reply(bash: &str, fx: &Fixture, cur: &str, want: &str) {
    let run = run_bash_cfg_path(bash, fx, cur);
    assert_eq!(run.rc, "0", "{bash} {cur}: {}", run.stderr);
    assert_eq!(run.reply, vec![want.to_string()], "{bash} {cur}");
}

/// Readline replaces the text after an open `"` and closes it, so the reply is quoted for the
/// inside of `"…"`: a `\ ` there would be a literal backslash.
#[test]
fn bash_path_reply_is_quoted_for_an_open_double_quote() {
    for bash in bashes() {
        let fx = fixture();
        for (cur, want) in [
            (r#""files/wi"#, "files/with space.txt"),
            (r#""files/it"#, "files/it's.txt"),
            (r#""files/q"#, r#"files/q\"d.txt"#),
        ] {
            assert_path_reply(&bash, &fx, cur, want);
        }
    }
}

/// Inside `'…'` nothing is escaped, and a `'` in the name closes, escapes and reopens the quote.
#[test]
fn bash_path_reply_is_quoted_for_an_open_single_quote() {
    for bash in bashes() {
        let fx = fixture();
        for (cur, want) in [
            ("'files/it", r"files/it'\''s.txt"),
            ("'files/wi", "files/with space.txt"),
            ("'files/q", r#"files/q"d.txt"#),
        ] {
            assert_path_reply(&bash, &fx, cur, want);
        }
    }
}

/// A quote opened mid-word is where readline starts replacing, so the reply drops what precedes it.
#[test]
fn bash_path_open_quote_mid_word_replaces_after_the_quote() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash_cfg_path(&bash, &fx, r#"files/"wi"#);
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert_eq!(run.reply, vec!["with space.txt".to_string()], "{bash}");
    }
}

/// A quoted `~` is a literal directory name, never the home directory.
#[test]
fn bash_path_quoted_tilde_is_literal() {
    for bash in bashes() {
        let fx = fixture();
        // `$HOME` is the fixture root, which holds `files/` but no `lit`.
        let run = run_bash_cfg_path(&bash, &fx, r#""~/fi"#);
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert!(run.reply.is_empty(), "{bash}: {:?}", run.reply);
        literal_dir(&fx, "~/lit");
        let run = run_bash_cfg_path(&bash, &fx, r#""~/l"#);
        assert_eq!(run.reply, vec!["~/lit/".to_string()], "{bash}");
        assert_eq!(run.compopt, vec!["-o nospace".to_string()], "{bash}");
    }
}

/// A `$` inside `'…'` is literal, so the substitution guard drops the word.
#[test]
fn bash_path_dollar_in_single_quotes_offers_nothing() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash_cfg_path(&bash, &fx, "'$HOME/fi");
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert!(run.reply.is_empty(), "{bash}: {:?}", run.reply);
    }
}

/// `$NAME` expands inside `"…"`, so it is read like a bare one and kept as typed.
#[test]
fn bash_path_variable_after_a_double_quote_stays_bare() {
    for bash in bashes() {
        let fx = fixture();
        for var in ["$HOME", "${HOME}"] {
            let cur = format!("\"{var}/files/s");
            let run = run_bash_cfg_path(&bash, &fx, &cur);
            assert_eq!(run.rc, "0", "{bash} {cur}: {}", run.stderr);
            assert_eq!(run.reply, vec![format!("{var}/files/sub/")], "{bash} {cur}");
            assert_eq!(run.compopt, vec!["-o nospace".to_string()], "{bash} {cur}");
        }
    }
}

/// Choice values are quoted for the open quote too.
#[test]
fn bash_value_reply_is_quoted_for_an_open_quote() {
    for bash in bashes() {
        let fx = fixture();
        for (cur, want) in [(r#""dry r"#, "dry run"), ("'dry r", "dry run")] {
            let run = run_bash(&bash, &fx, cur);
            assert_eq!(run.rc, "0", "{bash} {cur}: {}", run.stderr);
            assert_eq!(run.reply, vec![want.to_string()], "{bash} {cur}");
        }
    }
}

/// Dotfiles are listed with no leading `.` typed, whatever the user's `GLOBIGNORE`.
#[test]
fn bash_path_lists_dotfiles() {
    let script = print_script("bash");
    let ignore_all = format!("GLOBIGNORE='*'\n{script}");
    for bash in bashes() {
        let fx = fixture();
        let hid = fx.tmp.path().join("hid");
        std::fs::create_dir_all(&hid).unwrap();
        std::fs::write(hid.join(".env"), "").unwrap();
        std::fs::write(hid.join("config"), "").unwrap();
        for script in [&script, &ignore_all] {
            let run = run_bash_path_split(&bash, script, &fx, "--cfg", "hid/", Split::Whole);
            assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
            let got = run.sorted_reply();
            assert_eq!(got, vec!["hid/.env", "hid/config"], "{bash}");
        }
    }
}

/// `.` and `..` are offered only under a directory that exists.
#[test]
fn bash_path_dot_entries_need_an_existing_directory() {
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash_cfg_path(&bash, &fx, "nosuch/.");
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert!(run.reply.is_empty(), "{bash}: {:?}", run.reply);
        let run = run_bash_cfg_path(&bash, &fx, "files/.");
        let got = run.sorted_reply();
        assert_eq!(got, vec!["files/../", "files/./"], "{bash}");
    }
}

/// A typed `~/` or `$NAME/` stays bare even when the rest needs `$'…'` quoting, which
/// would otherwise stop the tilde expanding.
#[test]
fn bash_path_prefix_stays_bare_before_ansi_c_quoting() {
    for bash in bashes() {
        let fx = fixture();
        std::fs::write(fx.tmp.path().join("files/nl\nx"), "").unwrap();
        for (cur, want) in [
            ("~/files/nl", r"~/files/nl$'\n'x"),
            ("$HOME/files/nl", r"$HOME/files/nl$'\n'x"),
        ] {
            assert_path_reply(&bash, &fx, cur, want);
        }
    }
}

/// `compgen` lists `~user/` one name per line, so the tail of a name holding a newline is not a
/// candidate. A `compgen` function stands in for a real user's home, which a test can't write to.
#[test]
fn bash_tilde_user_drops_the_tail_of_a_newline_name() {
    let script = format!(
        "compgen() {{ [[ $1 == -d ]] && return 1; printf '%s\\n' '~fakeuser/nl' 'x' '~fakeuser/ok'; }}\n{}",
        print_script("bash")
    );
    for bash in bashes() {
        let fx = fixture();
        let run = run_bash_path_split(&bash, &script, &fx, "--cfg", "~fakeuser/", Split::Whole);
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert_eq!(run.reply, vec!["~fakeuser/nl", "~fakeuser/ok"], "{bash}");
    }
}

/// A name holding a newline is one candidate, not two.
#[test]
fn bash_path_name_with_a_newline_is_one_candidate() {
    for bash in bashes() {
        let fx = fixture();
        std::fs::write(fx.tmp.path().join("files/nl\nx"), "").unwrap();
        let run = run_bash_cfg_path(&bash, &fx, "files/nl");
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert_eq!(run.reply, vec![r"files/nl$'\n'x".to_string()], "{bash}");
    }
}

/// Names holding control characters, added under `files/` for the ANSI-C (`$'…'`) tests. Kept
/// out of [`PATH_FILES`] so the other tests' listings of `files/` don't change.
const CONTROL_FILES: [&str; 6] = [
    "files/an\nd",
    "files/an\nx",
    "files/nl\nx",
    "files/nl\tz",
    "files/solo\nname",
    "files/tab\there",
];

/// [`fixture`] plus [`CONTROL_FILES`] and a directory `files/nl\nd/` holding `in`.
fn control_fixture() -> Fixture {
    let fx = fixture();
    for name in CONTROL_FILES {
        std::fs::write(fx.tmp.path().join(name), "").unwrap();
    }
    let dir = fx.tmp.path().join("files/nl\nd");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("in"), "").unwrap();
    fx
}

/// Names that run a command, or reach the wrong argument, if a reply's quoting goes wrong. A
/// trailing `/` makes a directory. Each `touch` names a sentinel relative to the fixture root, the
/// shell's working directory: a file name can't hold the absolute path `SENTINEL` stands for.
const HOSTILE_FILES: [&str; 16] = [
    // A quote before a substitution, or before a control character, inside `$'…'`.
    "evil/q'$(touch sentinel-one)",
    "evil/b'`touch sentinel-two`",
    "evil/sq2'\u{1}",
    "evil/dirs'/in",
    // A reply starting with the quote the user opened.
    "lead/ctl/\u{1};touch sentinel-one;#",
    "lead/sub/\u{1}$(touch sentinel-one)",
    "lead/bang/!;touch sentinel-one;#",
    "lead/sq/';touch sentinel-one;#",
    "rq/r'",
    "rq/r''x",
    // A `$'…'` run closing as `\\'` before a break.
    "evb/b\\:z",
    "evb/b\\:y$(touch sentinel-one)",
    // `insert-completions` inside an open quote.
    "w/a;touch sentinel-one;#",
    "w/cx$(touch sentinel-one)",
    "d/'d1/",
    "d/'d2$(touch sentinel-one)/",
];

/// [`fixture`] plus [`HOSTILE_FILES`].
fn hostile_fixture() -> Fixture {
    let fx = fixture();
    for name in HOSTILE_FILES {
        let path = fx.tmp.path().join(name);
        if name.ends_with('/') {
            std::fs::create_dir_all(&path).unwrap();
        } else {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
        }
    }
    fx
}

/// Asserts no sentinel exists: a command that ran is the finding, whatever the reply.
fn assert_no_sentinels(fx: &Fixture, ctx: &str) {
    assert!(!fx.sentinel.exists(), "{ctx}: sentinel-one command ran");
    assert!(!fx.sentinel2.exists(), "{ctx}: sentinel-two command ran");
    assert!(!fx.sentinel3.exists(), "{ctx}: sentinel-three command ran");
}

/// Asserts completing the path `cur` after `--cfg` with `script` offers exactly `want`, in any
/// order.
fn assert_path_replies(bash: &str, script: &str, fx: &Fixture, cur: &str, want: &[&str]) {
    let run = run_bash_path_split(bash, script, fx, "--cfg", cur, Split::Whole);
    assert_eq!(run.rc, "0", "{bash} {cur}: {}", run.stderr);
    let mut want: Vec<String> = want.iter().map(|w| (*w).to_string()).collect();
    want.sort();
    assert_eq!(run.sorted_reply(), want, "{bash} {cur}");
}

/// Readline replaces the text after the `'` of an open `$'`, so the reply is escaped for the
/// inside of the run: a newline is `\n` there.
#[test]
fn bash_path_word_in_an_open_ansi_c_quote_is_listed() {
    let script = print_script("bash");
    for bash in bashes() {
        let fx = control_fixture();
        for (cur, want) in [
            (r"$'files/an", &[r"files/an\nd", r"files/an\nx"][..]),
            (r"$'files/solo\nna", &[r"files/solo\nname"][..]),
        ] {
            assert_path_replies(&bash, &script, &fx, cur, want);
        }
    }
}

/// The text before the open `$'` stays as typed, so the reply holds only what follows it.
#[test]
fn bash_path_open_ansi_c_quote_after_a_prefix_replies_only_the_rest() {
    for bash in bashes() {
        let fx = control_fixture();
        for (cur, want) in [
            (r"~/$'files/solo", r"files/solo\nname"),
            (r"$HOME/$'files/solo", r"files/solo\nname"),
            (r"files/$'solo", r"solo\nname"),
        ] {
            assert_path_reply(&bash, &fx, cur, want);
        }
    }
}

/// A closed `$'…'`, as toolr itself inserts it, decodes to the name, so Tab keeps completing
/// under it.
#[test]
fn bash_path_closed_ansi_c_word_keeps_completing() {
    let script = format!("TOOLR_TEST_DIR=$PWD\n{}", print_script("bash"));
    for bash in bashes() {
        let fx = control_fixture();
        for (cur, want) in [
            (r"$'files/nl\nd'/", r"files/nl$'\n'd/in"),
            (r"~/$'files/nl\nd'/", r"~/files/nl$'\n'd/in"),
            (r"$HOME/$'files/nl\nd'/", r"$HOME/files/nl$'\n'd/in"),
            (
                r"$TOOLR_TEST_DIR/$'files/nl\nd'/",
                r"$TOOLR_TEST_DIR/files/nl$'\n'd/in",
            ),
            (r"files/$'nl\nd'/", r"files/nl$'\n'd/in"),
        ] {
            assert_path_replies(&bash, &script, &fx, cur, &[want]);
        }
    }
}

/// Each escape bash decodes in `$'…'` stands for the same character here.
#[test]
fn bash_path_ansi_c_escapes_decode() {
    let script = print_script("bash");
    let utf8 = format!("LC_ALL=C.UTF-8\n{script}");
    for bash in bashes() {
        let fx = control_fixture();
        std::fs::write(fx.tmp.path().join("files/é.txt"), "").unwrap();
        for (cur, want) in [
            (r"$'files/tab\x09h", r"files/tab\there"),
            (r"$'files/tab\x9h", r"files/tab\there"),
            (r"$'files/tab\011h", r"files/tab\there"),
            (r"$'files/tab\th", r"files/tab\there"),
            (r"$'files/\x74ab\th", r"files/tab\there"),
            (r"$'files/it\047s", r"files/it\047s.txt"),
            (r#"$'files/q\"d"#, r#"files/q"d.txt"#),
            (r"$'files/back\\s", r"files/back\\slash"),
            (r"$'files/\u0074ab\th", r"files/tab\there"),
            // A lone backslash at the end is an escape still being typed.
            (r"$'files/solo\", r"files/solo\nname"),
        ] {
            assert_path_replies(&bash, &script, &fx, cur, &[want]);
        }
        assert_path_replies(&bash, &script, &fx, "$'files/é", &["files/é.txt"]);
        for cur in [r"$'files/\u00e9", r"$'files/\U000000e9"] {
            assert_path_replies(&bash, &utf8, &fx, cur, &["files/é.txt"]);
            // Outside a UTF-8 locale bash can't decode it, so nothing is offered.
            let c = format!("LC_ALL=C\n{script}");
            assert_path_replies(&bash, &c, &fx, cur, &[]);
        }
    }
}

/// An escape bash would keep as typed, or one decoding to NUL, which no name holds, offers
/// nothing, before toolr is even asked.
#[test]
fn bash_path_undecodable_ansi_c_escape_offers_nothing() {
    let script = format!(
        "toolr() {{ : > \"$PWD/engine-called\"; command toolr \"$@\"; }}\n{}",
        print_script("bash")
    );
    for bash in bashes() {
        let fx = control_fixture();
        for cur in [
            r"$'files/\q",
            r"$'files/nl\x00",
            r"$'files/\0",
            r"$'files/\cA",
            r"$'files/\x",
            r"$'files/\u0000",
            r"$'files/\400",
            // Readline sees `$'…'` as `'…'`, where `\'` closes the quote: the two would disagree.
            r"$'files/it\'s",
            r"$'files/it\'s'/",
        ] {
            assert_path_replies(&bash, &script, &fx, cur, &[]);
            let called = fx.tmp.path().join("engine-called");
            assert!(!called.exists(), "{bash} {cur}: toolr was asked");
        }
    }
}

/// A `$` or backtick spelled as an escape is still one: the guard sees the decoded word.
#[test]
fn bash_path_ansi_c_word_never_runs_a_substitution() {
    const SUB: &str = "$(touch sentinel-one)";
    const TICK: &str = "`touch sentinel-two`";
    let script = print_script("bash");
    for bash in bashes() {
        let fx = hostile_fixture();
        literal_dir(&fx, SUB);
        literal_dir(&fx, TICK);
        for cur in [
            "$'$(touch sentinel-one)",
            "$'$(touch sentinel-one)'/",
            r"$'\x24(touch sentinel-one)'/",
            r"$'\x24(touch sentinel-one)",
            r"$'\044(touch sentinel-one)'/",
            r"$'\u0024(touch sentinel-one)'/",
            r"~/$'\x24(touch sentinel-one)'/",
            r"$HOME/$'\044(touch sentinel-one)'/",
            r"~$'\x24(touch sentinel-one)'/",
            r"--cfg=$'\x24(touch sentinel-one)'/",
            r"$'\x60touch sentinel-two\x60'/",
            r"$'\140touch sentinel-two\140'/",
            "$'`touch sentinel-two`'/",
        ] {
            let run = run_bash_path_split(&bash, &script, &fx, "--cfg", cur, Split::Whole);
            // Checked first: a command that ran is the finding, whatever the reply.
            assert!(!fx.sentinel.exists(), "{bash} {cur}: $(...) ran");
            assert!(!fx.sentinel2.exists(), "{bash} {cur}: backtick ran");
            assert_eq!(run.rc, "0", "{bash} {cur}: {}", run.stderr);
            assert!(run.reply.is_empty(), "{bash} {cur}: {:?}", run.reply);
        }
        for cur in [r"$'\x24(touch sentinel-one)", r"$'\x60touch sentinel-two"] {
            let run = run_bash(&bash, &fx, cur);
            assert!(!fx.sentinel.exists(), "{bash} --mode {cur}: $(...) ran");
            assert!(!fx.sentinel2.exists(), "{bash} --mode {cur}: backtick ran");
            assert_eq!(run.rc, "0", "{bash} --mode {cur}: {}", run.stderr);
        }
        // A name holding a quote and a substitution: the reply must keep readline's idea of the
        // open quote in step with the script's, or readline inserts it unquoted.
        for (cur, want) in [
            ("$'evil/q", r"evil/q\047$(touch sentinel-one)"),
            ("$'evil/b", r"evil/b\047`touch sentinel-two`"),
            ("'evil/q", r"evil/q'\''$(touch sentinel-one)"),
            (r#""evil/q"#, r"evil/q'\$(touch sentinel-one)"),
            ("evil/q", r"evil/q\'\$\(touch\ sentinel-one\)"),
            ("$'evil/sq2", r"evil/sq2\047\001"),
            ("$'evil/dir", r"evil/dirs\047/"),
        ] {
            assert_path_replies(&bash, &script, &fx, cur, &[want]);
            assert_no_sentinels(&fx, &format!("{bash} {cur}"));
        }
    }
}

/// Bash expands a variable only where its `$` and whole name are typed bare: a decoded `$`, or
/// a name with a quoted part, is literal text, so it gets no variable treatment.
#[test]
fn bash_path_decoded_dollar_is_not_a_variable() {
    let script = format!("TOOLR_TEST_DIR=$PWD\n{}", print_script("bash"));
    for bash in bashes() {
        let fx = control_fixture();
        for cur in [
            r"$'\x24TOOLR_TEST_DIR/files/an",
            r"$'\x24TOOLR_TEST_DIR'/files/an",
            r"$TOOLR_TEST_D$'IR'/files/an",
            r#"$TOOLR_TEST_D"IR"/files/an"#,
            r"$TOOLR_TEST_D\IR/files/an",
        ] {
            assert_path_replies(&bash, &script, &fx, cur, &[]);
        }
        assert_path_replies(
            &bash,
            &script,
            &fx,
            r"$TOOLR_TEST_DIR/$'files/an",
            &[r"files/an\nd", r"files/an\nx"],
        );
    }
}

/// Bash expands `~` only with nothing quoted before the first `/`, and only `compgen` resolves
/// `~user`, so a tilde word with a quoted part must never reach it.
#[test]
fn bash_path_tilde_with_a_quoted_part_never_reaches_compgen() {
    let script = format!(
        "compgen() {{ : > \"$PWD/compgen-called\"; builtin compgen \"$@\"; }}\n{}",
        print_script("bash")
    );
    for bash in bashes() {
        let fx = fixture();
        for cur in [
            r#"~"root"/"#,
            "~'root'/",
            r"~\root/",
            r"~$'root'/",
            r"~$'\x72oot'/",
            r#"~r"oot"/"#,
        ] {
            assert_path_replies(&bash, &script, &fx, cur, &[]);
            let called = fx.tmp.path().join("compgen-called");
            assert!(!called.exists(), "{bash} {cur}: compgen was called");
        }
    }
}

/// A closed reply quotes each control character as its own `$'…'` run, so the common prefix
/// readline inserts can't end inside one.
#[test]
fn bash_path_bare_reply_groups_each_control_character() {
    let script = print_script("bash");
    for bash in bashes() {
        let fx = control_fixture();
        for (cur, want) in [
            ("files/an", &[r"files/an$'\n'd", r"files/an$'\n'x"][..]),
            (
                "files/nl",
                &[r"files/nl$'\n'd/", r"files/nl$'\n'x", r"files/nl$'\t'z"][..],
            ),
            (
                r"files/an$'\n'",
                &[r"files/an$'\n'd", r"files/an$'\n'x"][..],
            ),
        ] {
            assert_path_replies(&bash, &script, &fx, cur, want);
        }
        // Readline doesn't break at the `$` of `$'`, even with `$` in `COMP_WORDBREAKS`.
        let dollar = format!("COMP_WORDBREAKS+='$'\n{script}");
        assert_path_replies(
            &bash,
            &dollar,
            &fx,
            r"files/an$'\n'",
            &[r"files/an$'\n'd", r"files/an$'\n'x"],
        );
    }
}

/// The longest common prefix readline would insert.
fn common_prefix(replies: &[String]) -> String {
    let first = replies.first().cloned().unwrap_or_default();
    replies.iter().fold(first, |acc, r| {
        acc.chars()
            .zip(r.chars())
            .take_while(|(a, b)| a == b)
            .map(|(a, _)| a)
            .collect()
    })
}

/// The common prefix of grouped replies closes every `$'` it opens, except for names that
/// differ at the control character itself: there it ends inside the run, which is accepted.
#[test]
fn bash_path_common_prefix_of_grouped_replies_is_closed() {
    for bash in bashes() {
        let fx = control_fixture();
        let run = run_bash_cfg_path(&bash, &fx, "files/an");
        let prefix = common_prefix(&run.reply);
        assert_eq!(prefix, r"files/an$'\n'", "{bash}: {:?}", run.reply);
        assert_eq!(prefix.matches("$'").count(), 1, "{bash}");
        assert!(prefix.ends_with(r"\n'"), "{bash}");
        // `nl\nd`, `nl\nx` and `nl\tz` differ at the control character: the documented residual.
        let run = run_bash_cfg_path(&bash, &fx, "files/nl");
        assert_eq!(run.reply.len(), 3, "{bash}: {:?}", run.reply);
        assert_eq!(common_prefix(&run.reply), r"files/nl$'\", "{bash}");
    }
}

/// Inside an open `"` or `'` a raw newline can't be inserted, so the reply closes the quote,
/// adds the `$'…'` run and reopens it.
#[test]
fn bash_path_controls_in_open_double_and_single_quotes_close_and_reopen() {
    let script = print_script("bash");
    for bash in bashes() {
        let fx = control_fixture();
        for (cur, want) in [
            (
                r#""files/an"#,
                &[r#"files/an"$'\n'"d"#, r#"files/an"$'\n'"x"#][..],
            ),
            ("'files/an", &[r"files/an'$'\n''d", r"files/an'$'\n''x"][..]),
            (r#""files/ta"#, &[r#"files/tab"$'\t'"here"#][..]),
        ] {
            assert_path_replies(&bash, &script, &fx, cur, want);
        }
        // A C1 control is one character in a UTF-8 locale but two bytes, each escaped.
        std::fs::write(fx.tmp.path().join("files/c1\u{85}x"), "").unwrap();
        let utf8 = format!("LC_ALL=C.UTF-8\n{script}");
        for (cur, want) in [
            (r#""files/c1"#, r#"files/c1"$'\302\205'"x"#),
            ("'files/c1", r"files/c1'$'\302\205''x"),
            ("$'files/c1", r"files/c1\302\205x"),
            ("files/c1", r"files/c1$'\302\205'x"),
        ] {
            assert_path_replies(&bash, &utf8, &fx, cur, &[want]);
        }
    }
}

/// Readline closes the open quote after a single reply only if the reply doesn't already end in
/// the quote character, so a reply that does gets one more to close it.
#[test]
fn bash_path_reply_ending_in_the_open_quote_still_closes() {
    let script = print_script("bash");
    for bash in bashes() {
        let fx = fixture();
        let dir = fx.tmp.path().join("end");
        std::fs::create_dir_all(dir.join("dir'")).unwrap();
        for name in ["sq'", "nl\n", "dq\""] {
            std::fs::write(dir.join(name), "").unwrap();
        }
        for (cur, want) in [
            ("$'end/sq", r"end/sq\047"),
            ("'end/sq", r"end/sq'\'''"),
            (r#""end/dq"#, r#"end/dq\"""#),
            (r#""end/nl"#, r#"end/nl"$'\n'"""#),
            ("'end/nl", r"end/nl'$'\n'''"),
            ("$'end/nl", r"end/nl\n"),
            // A directory reply ends in `/`, so readline closes the quote itself.
            ("$'end/di", r"end/dir\047/"),
            ("'end/di", r"end/dir'\''/"),
        ] {
            assert_path_replies(&bash, &script, &fx, cur, &[want]);
        }
    }
}

/// A name equal to the text before an open quote leaves nothing to insert after it: a file is
/// not offered, and a directory offers only its `/`.
#[test]
fn bash_path_exact_match_under_an_open_quote_is_not_an_empty_reply() {
    let script = print_script("bash");
    for bash in bashes() {
        let fx = fixture();
        let ex = fx.tmp.path().join("ex");
        std::fs::create_dir_all(&ex).unwrap();
        std::fs::write(ex.join("exact"), "").unwrap();
        for q in ["'", "\"", "$'"] {
            assert_path_replies(&bash, &script, &fx, &format!("ex/exact{q}"), &[]);
            assert_path_replies(&bash, &script, &fx, &format!("files/sub{q}"), &["/"]);
        }
        std::fs::write(ex.join("exact2"), "").unwrap();
        for q in ["'", "\"", "$'"] {
            assert_path_replies(&bash, &script, &fx, &format!("ex/exact{q}"), &["2"]);
        }
        for cur in ["fast'", "fast\"", "fast$'"] {
            let run = run_bash(&bash, &fx, cur);
            assert_eq!(run.rc, "0", "{bash} {cur}: {}", run.stderr);
            assert!(run.reply.is_empty(), "{bash} {cur}: {:?}", run.reply);
        }
    }
}

/// Choice values decode the same way. The engine never offers a value holding a control
/// character, so only the decoding is exercised here.
#[test]
fn bash_value_completes_in_ansi_c_quotes() {
    for bash in bashes() {
        let fx = fixture();
        for (cur, want) in [
            ("$'dry r", "dry run"),
            (r"$'\x64ry r", "dry run"),
            ("$'wo", "wow!"),
            ("$'a:", "a:b"),
        ] {
            let run = run_bash(&bash, &fx, cur);
            assert_eq!(run.rc, "0", "{bash} {cur}: {}", run.stderr);
            assert_eq!(run.reply, vec![want.to_string()], "{bash} {cur}");
        }
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
        // The new binary's bare values don't start with the typed `--mode=`, so nothing matches.
        for split in SPLITS {
            let run = Drive::new(OLD_BASH_SCRIPT, &[], "--mode=dr")
                .split(split)
                .legacy()
                .run(&bash, &fx);
            assert_eq!(run.rc, "0", "{bash} {split:?}: {}", run.stderr);
            assert_eq!(run.stderr, "", "{bash} {split:?}");
            assert!(run.reply.is_empty(), "{bash} {split:?}: {:?}", run.reply);
        }
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
        let rc = bash_tab_rc(&fx);
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
        // Without `=`, readline replaces the whole `--opt=…` word.
        let no_equals_rcfile = fx.tmp.path().join("bashrc-no-equals");
        std::fs::write(
            &no_equals_rcfile,
            format!("{rc}COMP_WORDBREAKS=$' \\t\\n\"\\'@><;|&(:'\n"),
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
            (
                &rcfile,
                "toolr ci lint --mode=fa",
                "LINE:ci lint --mode=fast",
            ),
            (
                &rcfile,
                "toolr ci lint --mode=x=~r",
                "LINE:ci lint --mode=x=~root",
            ),
            (
                &rcfile,
                "toolr ci lint --cfg=files/host:p",
                "LINE:ci lint --cfg=files/host:port.txt",
            ),
            (
                &no_equals_rcfile,
                "toolr ci lint --mode=fa",
                "LINE:ci lint --mode=fast",
            ),
            (
                &no_equals_rcfile,
                "toolr ci lint --cfg=files/w",
                "LINE:ci lint --cfg=files/with space.txt",
            ),
        ] {
            assert_bash_tab_line(&bash, &fx, rcfile, typed, want);
        }
    }
}

/// The rcfile body for the pty tests: it sources the script, and a `toolr()` shim prints
/// each submitted line, after shell parsing, as `LINE:<args>`.
fn bash_tab_rc(fx: &Fixture) -> String {
    let script = fx.tmp.path().join("toolr.bash");
    std::fs::write(&script, print_script("bash")).unwrap();
    format!(
        "source '{}'\n\
         toolr() {{ if [[ ${{1-}} == __complete ]]; then command toolr \"$@\"; \
         else printf 'LINE:%s\\n' \"$*\"; fi; }}\n",
        script.display()
    )
}

/// Types `typed`, Tab and Enter into an interactive bash on a pty, and asserts the shim printed
/// `want`.
fn assert_bash_tab_line(bash: &str, fx: &Fixture, rcfile: &Path, typed: &str, want: &str) {
    assert_bash_keys_line(bash, fx, rcfile, &format!("{typed}\t"), want);
}

/// Types `keys` and Enter into an interactive bash on a pty, and asserts the shim printed `want`.
fn assert_bash_keys_line(bash: &str, fx: &Fixture, rcfile: &Path, typed: &str, want: &str) {
    let mut cmd = StdCommand::new(bash);
    cmd.args(["--noprofile", "--rcfile"])
        .arg(rcfile)
        .arg("-i")
        .current_dir(fx.tmp.path())
        // As in `Drive::run`: a stale inherited `PWD` makes a conda bash 5.1 call `getcwd`.
        .env("PWD", fx.tmp.path())
        .env("PATH", path_with_toolr())
        .env("HOME", fx.tmp.path())
        .env("XDG_CACHE_HOME", common::test_cache_home())
        // No escape sequences, and no user readline settings.
        .env("TERM", "dumb")
        .env("INPUTRC", "/dev/null");
    let out = common::run_on_pty(cmd, &format!("{typed}\nexit\n"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stdout.lines().any(|l| l.trim_end_matches('\r') == want),
        "{bash} {typed:?}: want {want:?}\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(!stderr.contains("command not found"), "{bash}: {stderr}");
}

/// Interactive bash history-expands a `!` even inside `"…"`, so a reply there must keep it
/// from doing so. The earlier `echo prev` gives `!!` something to expand to.
#[test]
fn bash_tab_keeps_a_bang_literal_in_double_quotes_end_to_end() {
    for bash in bashes() {
        let fx = fixture();
        let bang = fx.tmp.path().join("bang");
        std::fs::create_dir_all(&bang).unwrap();
        for name in ["h!!x", "x!y", "a!b", "a!c", "end!"] {
            std::fs::write(bang.join(name), "").unwrap();
        }
        std::fs::create_dir_all(bang.join("d!")).unwrap();
        std::fs::write(bang.join("d!/inner.txt"), "").unwrap();
        let rcfile = fx.tmp.path().join("bashrc");
        std::fs::write(&rcfile, format!("{}set -H\n", bash_tab_rc(&fx))).unwrap();
        for (typed, want) in [
            (r#"--cfg "bang/h"#, "--cfg bang/h!!x"),
            (r#"--cfg "bang/x"#, "--cfg bang/x!y"),
            // The first Tab inserts the common prefix `a"\!"`; the second completes after it.
            ("--cfg \"bang/a\tb", "--cfg bang/a!b"),
            (r#"--mode "wo"#, "--mode wow!"),
            (r#"--cfg "bang/e"#, "--cfg bang/end!"),
            // A directory reply ends in `/`, not `!`, so readline closes the quote itself.
            (r#"--cfg "bang/d"#, "--cfg bang/d!/"),
            (r#"--out "bang/d"#, "--out bang/d!/"),
            ("--cfg \"bang/d\t", "--cfg bang/d!/inner.txt"),
        ] {
            let typed = format!("echo prev\ntoolr ci lint {typed}");
            let want = format!("LINE:ci lint {want}");
            assert_bash_tab_line(&bash, &fx, &rcfile, &typed, &want);
        }
    }
}

/// Inside a live completion readline dequotes the word `compgen` gets, and it replaces only the
/// text after an open quote, so a backslash or quote in the typed path must survive both.
/// Words that should offer nothing are left out: with an open quote, Enter would wait for more.
#[test]
fn bash_tab_completes_quoted_and_escaped_paths_end_to_end() {
    for bash in bashes() {
        let fx = fixture();
        let rcfile = fx.tmp.path().join("bashrc");
        std::fs::write(&rcfile, bash_tab_rc(&fx)).unwrap();
        let home = fx.tmp.path().display().to_string();
        // `{home}` stands for the fixture root, which is `$HOME`.
        for (typed, want) in [
            (r"--cfg files/back\\s", r"--cfg files/back\slash"),
            (r"--cfg files/it\'", "--cfg files/it's.txt"),
            (r#"--cfg "files/wi"#, "--cfg files/with space.txt"),
            ("--cfg 'files/wi", "--cfg files/with space.txt"),
            (r#"--cfg files/"wi"#, "--cfg files/with space.txt"),
            (r#"--cfg "files/it"#, "--cfg files/it's.txt"),
            ("--cfg 'files/it", "--cfg files/it's.txt"),
            ("--cfg 'files/q", r#"--cfg files/q"d.txt"#),
            (
                r#"--cfg "$HOME/files/wi"#,
                "--cfg {home}/files/with space.txt",
            ),
            (r"--cfg=files/back\\s", r"--cfg=files/back\slash"),
            (r#"--mode "dry r"#, "--mode dry run"),
            ("--cfg ~/files/wi", "--cfg {home}/files/with space.txt"),
        ] {
            let typed = format!("toolr ci lint {typed}");
            let want = format!("LINE:ci lint {}", want.replace("{home}", &home));
            assert_bash_tab_line(&bash, &fx, &rcfile, &typed, &want);
        }
    }
}

/// [`bash_tab_rc`] with a shim that prints each argument through `%q`, in the same `LINE:<args>`
/// shape, so an argument holding a newline stays on the `LINE:` line.
fn bash_tab_rc_quoted(fx: &Fixture) -> String {
    let script = fx.tmp.path().join("toolr.bash");
    std::fs::write(&script, print_script("bash")).unwrap();
    format!(
        "source '{}'\n\
         toolr() {{ if [[ ${{1-}} == __complete ]]; then command toolr \"$@\"; \
         else local a; printf -v a ' %q' \"$@\"; printf 'LINE:%s\\n' \"${{a# }}\"; fi; }}\n",
        script.display()
    )
}

/// The common prefix readline inserts for two names differing after a newline closes its `$'`,
/// so Enter runs the command instead of waiting for the quote to close.
#[test]
fn bash_tab_never_leaves_an_ansi_c_quote_open_end_to_end() {
    for bash in bashes() {
        let fx = control_fixture();
        let rcfile = fx.tmp.path().join("bashrc");
        std::fs::write(&rcfile, bash_tab_rc_quoted(&fx)).unwrap();
        let typed = "toolr ci lint --cfg files/an";
        let want = r"LINE:ci lint --cfg $'files/an\n'";
        assert_bash_tab_line(&bash, &fx, &rcfile, typed, want);
    }
}

/// Words in `$'…'`, open or closed, complete to the name they spell.
#[test]
fn bash_tab_completes_ansi_c_quoted_paths_end_to_end() {
    for bash in bashes() {
        let fx = control_fixture();
        let rcfile = fx.tmp.path().join("bashrc");
        std::fs::write(&rcfile, bash_tab_rc_quoted(&fx)).unwrap();
        let home = fx.tmp.path().display().to_string();
        // `{home}` stands for the fixture root, which is `$HOME`.
        for (typed, want) in [
            (r"--cfg $'files/so", r"--cfg $'files/solo\nname'"),
            (r"--cfg files/$'so", r"--cfg $'files/solo\nname'"),
            (r"--cfg $'files/solo\", r"--cfg $'files/solo\nname'"),
            (r"--cfg ~/$'files/so", r"--cfg $'{home}/files/solo\nname'"),
            (
                r"--cfg $HOME/$'files/so",
                r"--cfg $'{home}/files/solo\nname'",
            ),
            (
                r"--cfg ~/$'files/nl\nd'/",
                r"--cfg $'{home}/files/nl\nd/in'",
            ),
            (r"--cfg $'files/tab\x09", r"--cfg $'files/tab\there'"),
            // The first Tab inserts the common prefix; the second completes after it.
            ("--cfg files/an\td", r"--cfg $'files/an\nd'"),
            ("--cfg $'files/an\tx", r"--cfg $'files/an\nx'"),
            ("--cfg \"files/an\td", r"--cfg $'files/an\nd'"),
            ("--cfg 'files/an\tx", r"--cfg $'files/an\nx'"),
            ("--cfg files/nl\tnx", r"--cfg $'files/nl\nx'"),
        ] {
            let typed = format!("toolr ci lint {typed}");
            let want = format!("LINE:ci lint {}", want.replace("{home}", &home));
            assert_bash_tab_line(&bash, &fx, &rcfile, &typed, &want);
        }
        // `$` as a break doesn't split a closed `$'…'`: readline replaces the whole word.
        let dollar_rcfile = fx.tmp.path().join("bashrc-dollar");
        std::fs::write(
            &dollar_rcfile,
            format!(
                "{}COMP_WORDBREAKS=$' \\t\\n\"\\'@><=;|&(:$'\n",
                bash_tab_rc_quoted(&fx)
            ),
        )
        .unwrap();
        let typed = r"toolr ci lint --cfg files/an$'\n'd";
        let want = r"LINE:ci lint --cfg $'files/an\nd'";
        assert_bash_tab_line(&bash, &fx, &dollar_rcfile, typed, want);
    }
}

/// Readline closes an open quote after a reply that already ends in the quote character only
/// with the extra one the reply carries.
#[test]
fn bash_tab_closes_a_quote_after_a_reply_ending_in_it_end_to_end() {
    for bash in bashes() {
        let fx = fixture();
        let dir = fx.tmp.path().join("end");
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["sq'", "nl\n", "dq\""] {
            std::fs::write(dir.join(name), "").unwrap();
        }
        let rcfile = fx.tmp.path().join("bashrc");
        std::fs::write(&rcfile, bash_tab_rc_quoted(&fx)).unwrap();
        for (typed, want) in [
            ("--cfg $'end/sq", r"--cfg end/sq\'"),
            ("--cfg 'end/sq", r"--cfg end/sq\'"),
            ("--cfg \"end/dq", r#"--cfg end/dq\""#),
            ("--cfg \"end/nl", r"--cfg $'end/nl\n'"),
            ("--cfg 'end/nl", r"--cfg $'end/nl\n'"),
            ("--cfg $'end/nl", r"--cfg $'end/nl\n'"),
        ] {
            let typed = format!("toolr ci lint {typed}");
            let want = format!("LINE:ci lint {want}");
            assert_bash_tab_line(&bash, &fx, &rcfile, &typed, &want);
        }
    }
}

/// Readline knows only `'`, `"` and backslash, so inside `$'…'` it sees a plain `'…'`, where a
/// backslash is literal. A `'` in a reply there, or a typed `\'`, would close readline's quote
/// while the script's stays open, and readline would insert the rest of the name unquoted.
#[test]
fn bash_tab_never_runs_a_name_holding_a_quote_end_to_end() {
    for bash in bashes() {
        let fx = hostile_fixture();
        let rcfile = fx.tmp.path().join("bashrc");
        std::fs::write(&rcfile, bash_tab_rc_quoted(&fx)).unwrap();
        for (typed, want) in [
            ("--cfg $'evil/q", r"--cfg evil/q\'\$\(touch\ sentinel-one\)"),
            ("--cfg 'evil/q", r"--cfg evil/q\'\$\(touch\ sentinel-one\)"),
            ("--cfg \"evil/q", r"--cfg evil/q\'\$\(touch\ sentinel-one\)"),
            ("--cfg evil/q", r"--cfg evil/q\'\$\(touch\ sentinel-one\)"),
            ("--cfg $'evil/b", r"--cfg evil/b\'\`touch\ sentinel-two\`"),
            // A typed `\'` offers nothing; the `'` typed after the Tab closes the word for bash.
            ("--cfg $'evil/q\\'\t'", r"--cfg evil/q\'"),
            ("--cfg $'evil/sq2", r"--cfg $'evil/sq2\'\001'"),
            // The first Tab inserts the directory; the second completes inside it.
            ("--cfg $'evil/dir\t", r"--cfg evil/dirs\'/in"),
        ] {
            let typed = format!("toolr ci lint {typed}");
            let want = format!("LINE:ci lint {want}");
            assert_bash_tab_line(&bash, &fx, &rcfile, &typed, &want);
            assert_no_sentinels(&fx, &format!("{bash} {typed:?}"));
        }
    }
}

/// Readline's `insert_match` won't double an opening quote: a reply starting with the quote the
/// user opened overwrites it, so the reply's own quoting flips and the rest lands unquoted.
#[test]
fn bash_tab_reply_starting_with_the_open_quote_end_to_end() {
    for bash in bashes() {
        let fx = hostile_fixture();
        let sentinel = fx.sentinel.display().to_string();
        for history in ["set -H", "set +H"] {
            let quoted = fx.tmp.path().join("bashrc-quoted");
            std::fs::write(&quoted, format!("{}{history}\n", bash_tab_rc_quoted(&fx))).unwrap();
            let plain = fx.tmp.path().join("bashrc-plain");
            std::fs::write(&plain, format!("{}{history}\n", bash_tab_rc(&fx))).unwrap();
            for (rcfile, typed, want) in [
                (
                    &quoted,
                    "--cfg lead/ctl/'",
                    r"LINE:ci lint --cfg $'lead/ctl/\001;touch sentinel-one;#'",
                ),
                (
                    &quoted,
                    "--cfg lead/ctl/\"",
                    r"LINE:ci lint --cfg $'lead/ctl/\001;touch sentinel-one;#'",
                ),
                (
                    &quoted,
                    "--cfg lead/sub/'",
                    r"LINE:ci lint --cfg $'lead/sub/\001$(touch sentinel-one)'",
                ),
                (
                    &quoted,
                    "--cfg lead/sub/\"",
                    r"LINE:ci lint --cfg $'lead/sub/\001$(touch sentinel-one)'",
                ),
                (
                    &plain,
                    "--cfg lead/bang/\"",
                    "LINE:ci lint --cfg lead/bang/!;touch sentinel-one;#",
                ),
                (
                    &plain,
                    "--cfg lead/sq/'",
                    "LINE:ci lint --cfg lead/sq/';touch sentinel-one;#",
                ),
                // The first Tab inserts the common prefix, and Backspace (`^H`: a typed-ahead DEL
                // is erased by the pty's line discipline) reopens the quote.
                (&plain, "--cfg 'rq/r\t\x08", "LINE:ci lint --cfg rq/r''x"),
                (
                    &plain,
                    "--mode \"!",
                    &format!("LINE:ci lint --mode !;touch {sentinel};#"),
                ),
            ] {
                let typed = format!("toolr ci lint {typed}");
                assert_bash_tab_line(&bash, &fx, rcfile, &typed, want);
                assert_no_sentinels(&fx, &format!("{bash} {history} {typed:?}"));
            }
        }
    }
}

/// Readline misreads a `$'…'` run holding `\'`, or closing on a `'` right after a `\`, and its
/// idea of the open quote then differs from the script's. Such a line offers nothing.
#[test]
fn bash_path_ansi_c_forms_readline_misreads_offer_nothing() {
    let script = print_script("bash");
    for bash in bashes() {
        let fx = hostile_fixture();
        assert_path_replies(&bash, &script, &fx, r"$'evb/b\\':z", &[]);
        assert_path_replies(&bash, &script, &fx, r"$'evb/b\\'", &[]);
        // A misread word earlier on the line puts readline out of step for the rest of it.
        let run = Drive::new(&script, &["--out", r"$'a\'b'", "--cfg"], "lead/sq/").run(&bash, &fx);
        assert_eq!(run.rc, "0", "{bash}: {}", run.stderr);
        assert!(run.reply.is_empty(), "{bash}: {:?}", run.reply);
        let run =
            Drive::new(&script, &["--out", r"$'a\047b'", "--cfg"], "lead/sq/").run(&bash, &fx);
        assert_eq!(run.reply.len(), 1, "{bash}: {:?}", run.reply);
    }
}

/// End to end, the misread forms leave the typed text as it was.
#[test]
fn bash_tab_keeps_ansi_c_forms_readline_misreads_end_to_end() {
    for bash in bashes() {
        let fx = hostile_fixture();
        let rcfile = fx.tmp.path().join("bashrc");
        std::fs::write(&rcfile, bash_tab_rc(&fx)).unwrap();
        for (typed, want) in [
            (r"--cfg $'evb/b\\':z", r"LINE:ci lint --cfg evb/b\:z"),
            (
                r"--out $'a\'b' --cfg lead/sq/",
                "LINE:ci lint --out a'b --cfg lead/sq/",
            ),
        ] {
            let typed = format!("toolr ci lint {typed}");
            assert_bash_tab_line(&bash, &fx, &rcfile, &typed, want);
            assert_no_sentinels(&fx, &format!("{bash} {typed:?}"));
        }
    }
}

/// `insert-completions` (`M-*`, `COMP_TYPE` 42) deletes the open quote and inserts every reply as
/// is, so a reply quoted for the inside of the quote would land unquoted: offer nothing there.
#[test]
fn bash_insert_completions_inside_an_open_quote_offers_nothing() {
    let script = print_script("bash");
    let insert_all = format!("COMP_TYPE=42\n{script}");
    for bash in bashes() {
        let fx = hostile_fixture();
        for cur in ["'w/a", "\"w/a", "$'w/c", "$'d/", "w/$'c", "'d/"] {
            assert_path_replies(&bash, &insert_all, &fx, cur, &[]);
        }
        // With no open quote every reply is a whole quoted word, which is safe to insert.
        for cur in ["w/", "d/"] {
            let want = run_bash_cfg_path(&bash, &fx, cur);
            assert_eq!(want.reply.len(), 2, "{bash} {cur}: {:?}", want.reply);
            let want: Vec<&str> = want.reply.iter().map(String::as_str).collect();
            assert_path_replies(&bash, &insert_all, &fx, cur, &want);
        }
        assert_no_sentinels(&fx, &bash);
    }
}

/// End to end: `ESC *` inside an open quote inserts nothing, and the `'` typed after it closes the
/// word for bash; with no open quote it inserts each name quoted.
#[test]
fn bash_insert_completions_never_runs_a_name_end_to_end() {
    for bash in bashes() {
        let fx = hostile_fixture();
        let rcfile = fx.tmp.path().join("bashrc");
        std::fs::write(&rcfile, bash_tab_rc(&fx)).unwrap();
        for (keys, want) in [
            ("--cfg 'w/a\x1b*'", "LINE:ci lint --cfg w/a"),
            ("--cfg \"w/a\x1b*\"", "LINE:ci lint --cfg w/a"),
            ("--cfg $'w/c\x1b*'", "LINE:ci lint --cfg w/c"),
            ("--cfg $'d/\x1b*'", "LINE:ci lint --cfg d/"),
            (
                "--cfg w/a\x1b*",
                "LINE:ci lint --cfg w/a;touch sentinel-one;#",
            ),
        ] {
            let keys = format!("toolr ci lint {keys}");
            assert_bash_keys_line(&bash, &fx, &rcfile, &keys, want);
            assert_no_sentinels(&fx, &format!("{bash} {keys:?}"));
        }
    }
}

/// Bash doesn't history-expand a `!` inside `$'…'`, on 5.1 to 5.3, so a reply there leaves it
/// bare; outside it, as in a grouped reply, `%q` escapes it.
#[test]
fn bash_tab_keeps_a_bang_literal_in_ansi_c_quotes_end_to_end() {
    for bash in bashes() {
        let fx = fixture();
        let bang = fx.tmp.path().join("bang");
        std::fs::create_dir_all(&bang).unwrap();
        for name in ["h!!x", "end!", "b!\nz"] {
            std::fs::write(bang.join(name), "").unwrap();
        }
        let rcfile = fx.tmp.path().join("bashrc");
        std::fs::write(&rcfile, format!("{}set -H\n", bash_tab_rc_quoted(&fx))).unwrap();
        for (typed, want) in [
            ("--cfg $'bang/h", r"--cfg bang/h\!\!x"),
            ("--cfg bang/$'h", r"--cfg bang/h\!\!x"),
            ("--cfg $'bang/e", r"--cfg bang/end\!"),
            ("--cfg $'bang/b", r"--cfg $'bang/b!\nz'"),
            ("--cfg bang/b", r"--cfg $'bang/b!\nz'"),
            ("--mode $'wo", r"--mode wow\!"),
        ] {
            let typed = format!("echo prev\ntoolr ci lint {typed}");
            let want = format!("LINE:ci lint {want}");
            assert_bash_tab_line(&bash, &fx, &rcfile, &typed, &want);
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
    fish_complete_with(fx, "", line)
}

/// [`fish_complete`] with `prelude` run after the script is sourced.
fn fish_complete_with(fx: &Fixture, prelude: &str, line: &str) -> String {
    let out = StdCommand::new("fish")
        .arg("--no-config")
        .arg("-c")
        .arg("source (echo $argv[1] | psub); eval $argv[3]; complete -C $argv[2]")
        .arg(print_script("fish"))
        .arg(line)
        .arg(prelude)
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
        assert!(
            l.contains("-a candidates") || l.contains("-a shown_values"),
            "{l}"
        );
        assert!(!l.contains("-Q"), "{l}");
    }
    assert!(!script.contains("eval "), "zsh script must not eval values");
}

/// Runs the real `_toolr` under `zsh -f` with `_files` and `compadd` stubbed
/// to record their arguments; `words` completes the last word.
fn zsh_complete(fx: &Fixture, words: &[&str]) -> Vec<String> {
    zsh_run_with(fx, "", "", words)
        .into_iter()
        .filter(|l| !is_stub_detail(l))
        .collect()
}

/// A line the `compadd` stub prints for the candidates it was given, not the call itself.
fn is_stub_detail(line: &str) -> bool {
    ["VALUE:", "DISP:"].iter().any(|p| line.starts_with(p))
}

/// The `-d` display entries in `lines`, from the stub's `DISP:` lines.
fn disp_lines(lines: &[String]) -> Vec<&str> {
    lines
        .iter()
        .filter_map(|l| l.strip_prefix("DISP:"))
        .collect()
}

/// The `compadd` flags of a script that lists names with their descriptions.
const DESCRIBED_FLAGS: &str = "-l -d display -a shown_values";

/// The `compadd` flags of a script that lists the plain grid.
const GRID_FLAGS: &str = "-a candidates";

/// The stub's `COMPADD:` line for `flags`.
fn compadd_line(flags: &str) -> String {
    format!("COMPADD:{flags}")
}

/// The `display` array of a script run against a toolr that replies with `shim`'s output.
fn zsh_display_for(shim: &str) -> Vec<String> {
    let fx = fixture();
    let lines = zsh_run_with(&fx, "", shim, &["toolr", "x"]);
    disp_lines(&lines).into_iter().map(String::from).collect()
}

/// The stub driver: it sets `compstate` states a test needs, including ones live zsh never
/// produces; [`zsh_tab`] checks what real zsh does. `quote` is the quote character zsh reports
/// at the cursor and `prelude` runs after the script loads. The stub `compadd` prints the call,
/// then a `VALUE:` line for each element of the array its last argument names (the script's
/// `shown_values` or `candidates`), then a `DISP:` line for each element of the script's
/// `display`. It reads those by name through dynamic scope, so a rename in `zsh.zsh` must follow.
fn zsh_run_with(fx: &Fixture, quote: &str, prelude: &str, words: &[&str]) -> Vec<String> {
    zsh_run_script(fx, &print_script("zsh"), quote, prelude, words)
}

/// [`zsh_run_with`] for any `_toolr` source, such as one an older toolr installed.
fn zsh_run_script(
    fx: &Fixture,
    script: &str,
    quote: &str,
    prelude: &str,
    words: &[&str],
) -> Vec<String> {
    // The `compadd` stub reads the script's own locals by name: zsh functions see their caller's.
    const DRIVER: &str = r#"
compdef() { :; }
_files() { print -r -- "FILES:$*"; }
_description() { :; }
compadd() {
    # `-O` matches without adding; this stub's matcher accepts every candidate.
    if [[ $1 == -O ]]; then set -A "$2" "${(@P)${@[-1]}}"; return; fi
    print -r -- "COMPADD:$*"; print -rl -- "${(@P)${@[-1]}/#/VALUE:}"
    if (( ${#display} )); then print -rl -- "${display[@]/#/DISP:}"; fi
}
compset() { print -r -- "COMPSET:$*"; }
typeset -A compstate
compstate[quote]=$ZSH_QUOTE
eval "$1"; shift
eval "$ZSH_PRELUDE"
words=("$@"); CURRENT=$#
_toolr
"#;
    let out = StdCommand::new("zsh")
        .arg("-f")
        .arg("-c")
        .arg(DRIVER)
        .arg("driver")
        .arg(script)
        .args(words)
        .current_dir(fx.tmp.path())
        .env("PATH", path_with_toolr())
        .env("XDG_CACHE_HOME", common::test_cache_home())
        .env("ZSH_QUOTE", quote)
        .env("ZSH_PRELUDE", prelude)
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

/// The candidates `_toolr` hands `compadd`.
fn zsh_values(fx: &Fixture, quote: &str, words: &[&str]) -> Vec<String> {
    zsh_run_with(fx, quote, "", words)
        .iter()
        .filter_map(|l| l.strip_prefix("VALUE:").map(str::to_string))
        .collect()
}

/// The word `_toolr` hands the engine for the last of `words`, and the candidates it gets back.
/// The `toolr` shim writes the word to a file: its stdout is the script's `$(…)`, not ours.
fn zsh_engine_word(fx: &Fixture, quote: &str, words: &[&str]) -> (String, Vec<String>) {
    let shim = r#"toolr() { print -rn -- "${@[-1]}" > word.out; command toolr "$@"; }"#;
    let values = zsh_run_with(fx, quote, shim, words)
        .iter()
        .filter_map(|l| l.strip_prefix("VALUE:").map(str::to_string))
        .collect();
    let word = std::fs::read_to_string(fx.tmp.path().join("word.out")).unwrap();
    (word, values)
}

/// `$words` holds the word as typed; the script must take the quoting off before the engine
/// sees it. zsh reports the open quote even after it's closed, so `'dry r'` carries `'` too,
/// and reports none for a quote opened mid-word. The engine returns every choice and zsh
/// matches; [`zsh_tab_completes_quoted_and_escaped_partials_end_to_end`] checks what lands.
#[test]
fn zsh_quoted_and_escaped_partials_match_a_choice() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    for (quote, cur, want) in [
        ("", r"dry\ r", "dry r"),
        ("'", "'dry r", "dry r"),
        ("\"", "\"dry r", "dry r"),
        ("'", "'dry r'", "dry r"),
        ("$'", "$'dry r", "dry r"),
        ("", "dr'y r'", "dry r"),
        ("", "dr'y r", "dry r"),
        ("", "d\"ry r", "dry r"),
        // Both quote kinds, only one of them open.
        ("", "it\"'", "it'"),
        ("", "a\"b'c d", "ab'c d"),
    ] {
        let (word, values) = zsh_engine_word(&fx, quote, &["toolr", "ci", "lint", "--mode", cur]);
        assert_eq!(word, want, "{quote:?} {cur:?}");
        assert_eq!(values, sorted_choices(&fx), "{quote:?} {cur:?}");
    }
    let (word, values) = zsh_engine_word(&fx, "", &["toolr", "ci", "lint", "--mode='dry r"]);
    assert_eq!(word, "--mode=dry r");
    assert_eq!(values, sorted_choices(&fx));
}

#[test]
fn zsh_attached_option_with_escaped_value_matches() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    let calls = zsh_complete(&fx, &["toolr", "ci", "lint", r"--mode=dry\ r"]);
    assert_eq!(
        calls,
        vec!["COMPSET:-P 1 *=".to_string(), compadd_line(DESCRIBED_FLAGS),]
    );
    let (word, values) = zsh_engine_word(&fx, "", &["toolr", "ci", "lint", r"--mode=dry\ r"]);
    assert_eq!(word, "--mode=dry r");
    assert_eq!(values, sorted_choices(&fx));
}

/// A quoted earlier word must reach the engine as the shell will pass it, or `'--mode'` isn't
/// recognised as the option whose value is being completed (the slot would offer flag names).
#[test]
fn zsh_earlier_quoted_words_reach_the_engine_unquoted() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    let words = ["toolr", "ci", "lint", "'--mode'", "fa"];
    assert_eq!(zsh_values(&fx, "", &words), sorted_choices(&fx));
    let words = [
        "toolr", "ci", "lint", r"--cfg", r"a\ b", "--mode", r"dry\ r",
    ];
    assert_eq!(zsh_values(&fx, "", &words), sorted_choices(&fx));
    // Unquoted, the same position is a flag-name slot.
    let words = ["toolr", "ci", "lint", "--cfg", "a", "--zz"];
    assert!(zsh_values(&fx, "", &words).contains(&"--mode".to_string()));
}

/// Live zsh completes the command inside backticks itself and never calls `_toolr`; if it ever
/// did, the script offers nothing rather than treat a command as a value.
#[test]
fn zsh_backtick_quote_offers_nothing() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    let words = ["toolr", "ci", "lint", "--mode", "`dry"];
    assert_eq!(zsh_run_with(&fx, "`", "", &words), Vec::<String>::new());
}

/// A typed `$(...)` or backtick, quoted or not, is matched as text and never runs.
#[test]
fn zsh_quoted_substitution_partial_is_inert() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    let sub = format!("$(touch {})", fx.sentinel.display());
    let tick = format!("`touch {}`", fx.sentinel2.display());
    for (quote, cur, want) in [
        ("'", "'$(touch", "$(touch"),
        ("$'", "$'$(touch", "$(touch"),
        ("", r"\$\(touch", "$(touch"),
        ("'", "'`touch", "`touch"),
        ("", r"\`touch", "`touch"),
    ] {
        let (word, values) = zsh_engine_word(&fx, quote, &["toolr", "ci", "lint", "--mode", cur]);
        assert_eq!(word, want, "{quote:?} {cur:?}");
        assert!(
            values.contains(&sub) && values.contains(&tick),
            "{values:?}"
        );
    }
    // In `"…"`, `$(` opens a substitution; live zsh doesn't call `_toolr`, and the stub run
    // must not run it either.
    let words = ["toolr", "ci", "lint", "--mode", "\"$(touch"];
    zsh_values(&fx, "\"", &words);
    assert!(!fx.sentinel.exists(), "a typed $(...) ran");
    assert!(!fx.sentinel2.exists(), "a typed backtick ran");
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
    assert_eq!(calls, vec![compadd_line(DESCRIBED_FLAGS)]);
}

/// The bash script with `=` taken out of `COMP_WORDBREAKS`, and the breaks the driver splits at.
fn no_equals_wordbreaks() -> (String, String) {
    (
        format!(
            "COMP_WORDBREAKS=${{COMP_WORDBREAKS//=/}}\n{}",
            print_script("bash")
        ),
        WORDBREAKS.replace('=', ""),
    )
}

/// Completes the attached `--opt=value` word `cur` on its own after `toolr ci lint`;
/// [`BashRun::quoted`] holds the `%q` forms of `quote`.
fn run_attached(
    bash: &str,
    fx: &Fixture,
    (script, breaks): (&str, &str),
    cur: &str,
    split: Split,
    quote: Vec<String>,
) -> BashRun {
    Drive::new(script, &[], cur)
        .split(split)
        .breaks(breaks)
        .quote(quote)
        .run(bash, fx)
}

/// `%q` forms with every `~` escaped, as the script quotes on any bash.
fn tilde_escaped(quoted: &[String]) -> Vec<String> {
    quoted
        .iter()
        .map(|q| q.replace("\\~", "~").replace('~', "\\~"))
        .collect()
}

/// Readline replaces only the text after the `=`, so the reply is the bare value; without `=` in
/// `COMP_WORDBREAKS` it replaces the whole word, so the reply carries `--mode=` again.
#[test]
fn bash_attached_value_offers_the_choices() {
    let script = print_script("bash");
    let (no_eq_script, no_eq_breaks) = no_equals_wordbreaks();
    for bash in bashes() {
        for split in SPLITS {
            let fx = fixture();
            let with_eq = (script.as_str(), WORDBREAKS);
            let run = run_attached(&bash, &fx, with_eq, "--mode=", split, fx.choices());
            assert_eq!(run.rc, "0", "{bash} {split:?}: {}", run.stderr);
            assert_eq!(run.stderr, "", "{bash} {split:?}");
            let mut want = tilde_escaped(&run.quoted);
            want.sort();
            let got = run.sorted_reply();
            assert_eq!(got, want, "{bash} {split:?}");
            assert!(
                !fx.sentinel.exists(),
                "{bash} {split:?}: $(...) value executed"
            );
            assert!(
                !fx.sentinel2.exists(),
                "{bash} {split:?}: backtick value executed"
            );

            let quote = vec!["dry run".to_string(), "dry walk".to_string()];
            let run = run_attached(&bash, &fx, with_eq, "--mode=dr", split, quote.clone());
            assert_eq!(run.reply, run.quoted, "{bash} {split:?}");
            let run = run_attached(&bash, &fx, with_eq, "--mode=x=", split, vec![]);
            assert_eq!(run.reply, vec!["\\~root".to_string()], "{bash} {split:?}");
            let run = run_attached(&bash, &fx, with_eq, "--verbose=", split, vec![]);
            assert_eq!(run.rc, "0", "{bash} {split:?}: {}", run.stderr);
            assert!(run.reply.is_empty(), "{bash} {split:?}: {:?}", run.reply);

            let no_eq = (no_eq_script.as_str(), no_eq_breaks.as_str());
            let run = run_attached(&bash, &fx, no_eq, "--mode=fa", split, vec![]);
            assert_eq!(run.rc, "0", "{bash} {split:?}: {}", run.stderr);
            assert_eq!(
                run.reply,
                vec!["--mode=fast".to_string()],
                "{bash} {split:?}"
            );
            let run = run_attached(&bash, &fx, no_eq, "--mode=dr", split, quote);
            let want: Vec<String> = run.quoted.iter().map(|q| format!("--mode={q}")).collect();
            assert_eq!(run.reply, want, "{bash} {split:?}");
            let run = run_attached(&bash, &fx, no_eq, "--mode=x=", split, vec![]);
            assert_eq!(
                run.reply,
                vec!["--mode=x=\\~root".to_string()],
                "{bash} {split:?}"
            );
        }
    }
}

/// Path slots complete the part after `--cfg=`, keeping a typed `~` or `$NAME/` as typed; the
/// prefix comes back on every reply when readline replaces the whole word.
#[test]
fn bash_attached_path_completes_the_part_after_the_equals() {
    let script = print_script("bash");
    let (no_eq_script, no_eq_breaks) = no_equals_wordbreaks();
    let path_files: Vec<String> = PATH_FILES.iter().map(|f| (*f).to_string()).collect();
    for bash in bashes() {
        for split in SPLITS {
            for (lead, ctx) in [
                ("", (script.as_str(), WORDBREAKS)),
                ("--cfg=", (no_eq_script.as_str(), no_eq_breaks.as_str())),
            ] {
                let out_lead = lead.replace("cfg", "out");
                let fx = fixture();
                let at = format!("{bash} {split:?} {lead:?}");
                let run = run_attached(&bash, &fx, ctx, "--cfg=files/", split, path_files.clone());
                assert_eq!(run.rc, "0", "{at}: {}", run.stderr);
                assert_eq!(run.stderr, "", "{at}");
                let mut want: Vec<String> =
                    run.quoted.iter().map(|q| format!("{lead}{q}")).collect();
                want.push(format!("{lead}files/sub/"));
                want.sort();
                let got = run.sorted_reply();
                assert_eq!(got, want, "{at}");
                assert!(!fx.sentinel3.exists(), "{at}: file name executed");

                let run = run_attached(&bash, &fx, ctx, "--out=files/", split, vec![]);
                assert_eq!(run.reply, vec![format!("{out_lead}files/sub/")], "{at}");
                assert_eq!(run.compopt, vec!["-o nospace".to_string()], "{at}");
                let run = run_attached(&bash, &fx, ctx, "--cfg=files/w", split, vec![]);
                assert_eq!(
                    run.reply,
                    vec![format!("{lead}files/with\\ space.txt")],
                    "{at}"
                );
                // `:` is still a break, so readline keeps `--cfg=files/host:` either way.
                let run = run_attached(&bash, &fx, ctx, "--cfg=files/host:p", split, vec![]);
                assert_eq!(run.reply, vec!["port.txt".to_string()], "{at}");
                let run = run_attached(&bash, &fx, ctx, "--cfg=~/files/s", split, vec![]);
                assert_eq!(run.reply, vec![format!("{lead}~/files/sub/")], "{at}");
                for var in ["$HOME", "${HOME}"] {
                    let cur = format!("--cfg={var}/files/s");
                    let run = run_attached(&bash, &fx, ctx, &cur, split, vec![]);
                    assert_eq!(run.rc, "0", "{at} {var}: {}", run.stderr);
                    assert_eq!(
                        run.reply,
                        vec![format!("{lead}{var}/files/sub/")],
                        "{at} {var}"
                    );
                }
            }
        }
    }
}

/// A `$(...)` or backtick after `--cfg=` is never handed to `compgen`, which would run it.
#[test]
fn bash_attached_path_never_runs_a_typed_substitution() {
    let script = print_script("bash");
    let (no_eq_script, no_eq_breaks) = no_equals_wordbreaks();
    for bash in bashes() {
        for split in SPLITS {
            for ctx in [
                (script.as_str(), WORDBREAKS),
                (no_eq_script.as_str(), no_eq_breaks.as_str()),
            ] {
                let fx = fixture();
                for dir in ["$(touch sentinel-one)", "`touch sentinel-one`"] {
                    literal_dir(&fx, dir);
                    let cur = format!("--cfg={dir}/");
                    let run = run_attached(&bash, &fx, ctx, &cur, split, vec![]);
                    assert!(
                        !fx.sentinel.exists(),
                        "{bash} {split:?} {cur}: substitution ran"
                    );
                    assert_eq!(run.rc, "0", "{bash} {split:?} {cur}: {}", run.stderr);
                    assert!(
                        run.reply.is_empty(),
                        "{bash} {split:?} {cur}: {:?}",
                        run.reply
                    );
                }
            }
        }
    }
}

/// `compset -P 1 '*='` moves `--opt=` into `IPREFIX` so zsh matches and inserts the bare values.
#[test]
fn zsh_attached_value_moves_the_option_into_the_prefix() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    fn with(cur: &str) -> [&str; 4] {
        ["toolr", "ci", "lint", cur]
    }
    let compset = "COMPSET:-P 1 *=".to_string();
    assert_eq!(
        zsh_complete(&fx, &with("--mode=fa")),
        vec![compset.clone(), compadd_line(DESCRIBED_FLAGS)]
    );
    assert_eq!(
        zsh_complete(&fx, &with("--cfg=files/")),
        vec![compset.clone(), "FILES:".to_string()]
    );
    assert_eq!(
        zsh_complete(&fx, &with("--out=")),
        vec![compset.clone(), "FILES:-/".to_string()]
    );
    // No value slot: neither `compset` nor a blank `compadd` runs.
    for cur in ["--verbose=", "--nope="] {
        assert_eq!(zsh_complete(&fx, &with(cur)), Vec::<String>::new(), "{cur}");
    }
    // A partial no choice starts with still reaches `compadd`; zsh's matcher decides.
    assert_eq!(
        zsh_complete(&fx, &with("--mode=zz")),
        vec![compset, compadd_line(DESCRIBED_FLAGS)]
    );
    let calls = zsh_complete(&fx, &["toolr", "ci", "lint", "--mode", "fa"]);
    assert_eq!(calls, vec![compadd_line(DESCRIBED_FLAGS)]);
}

/// The first word of each candidate line `complete -C` printed.
fn fish_names(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .map(|l| l.split('\t').next().unwrap())
        .collect()
}

#[test]
fn fish_attached_value_keeps_the_option_prefix() {
    if !require("fish") {
        return;
    }
    let fx = fixture();
    let stdout = fish_complete(&fx, "toolr ci lint --mode=dr");
    assert_eq!(
        fish_names(&stdout),
        vec!["--mode=dry run", "--mode=dry walk"]
    );
    let stdout = fish_complete(&fx, "toolr ci lint --mode=");
    assert!(fish_names(&stdout).contains(&"--mode=x=~root"), "{stdout}");
    assert!(!fx.sentinel.exists());
    assert!(!fx.sentinel2.exists());

    let stdout = fish_complete(&fx, "toolr ci lint --cfg=files/");
    let names = fish_names(&stdout);
    for want in ["--cfg=files/with space.txt", "--cfg=files/sub/"] {
        assert!(names.contains(&want), "{want:?} not in {names:?}");
    }
    assert!(!fx.sentinel3.exists(), "fish executed a file name");
    let stdout = fish_complete(&fx, "toolr ci lint --out=files/");
    assert_eq!(fish_names(&stdout), vec!["--out=files/sub/"]);
}

/// How the interactive zsh in [`zsh_tab_with`] loads the completion script.
#[derive(Clone, Copy, Debug)]
enum ZshLoad {
    /// `source`d from the rc file, as the eval/source install modes do.
    Sourced,
    /// Saved as `_toolr` in an `fpath` directory and autoloaded by `compinit`, as
    /// `toolr self completion install zsh` leaves it.
    Autoloaded,
}

/// An interactive zsh on a pty: types `typed` after `toolr ci lint `, then Tab, then `after`.
/// The rc file sources the script, a `toolr()` shim prints each submitted line as `LINE:<args>`
/// after shell parsing, and `^Xd` prints the edit buffer as `BUF:<line>` and clears it.
fn zsh_tab(fx: &Fixture, typed: &str, after: &str) -> String {
    zsh_tab_with(fx, ZshLoad::Sourced, "", typed, after)
}

/// [`zsh_tab`] with the script loaded as `load` says and `rc` appended to the rc file, for
/// `zstyle` settings.
fn zsh_tab_with(fx: &Fixture, load: ZshLoad, rc: &str, typed: &str, after: &str) -> String {
    let input = format!("toolr ci lint {typed}\t{after}exit\n");
    let (stdout, _) = zsh_session(fx, load, rc, &input);
    stdout
}

/// Types `input` into the interactive zsh [`zsh_tab_with`] sets up; returns its stdout and what
/// it wrote to the terminal, where zsh draws the completion listing.
fn zsh_session(fx: &Fixture, load: ZshLoad, rc: &str, input: &str) -> (String, String) {
    let script = fx.tmp.path().join("toolr.zsh");
    let loader = match load {
        ZshLoad::Sourced => {
            std::fs::write(&script, print_script("zsh")).unwrap();
            format!(
                "autoload -Uz compinit\ncompinit -u -D\nsource '{}'\n",
                script.display()
            )
        }
        ZshLoad::Autoloaded => {
            let dir = fx.tmp.path().join("fpath");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("_toolr"), print_script("zsh")).unwrap();
            format!(
                "fpath=('{}' $fpath)\nautoload -Uz compinit\ncompinit -u -D\n",
                dir.display()
            )
        }
    };
    std::fs::write(
        fx.tmp.path().join(".zshrc"),
        format!(
            "{loader}\
             toolr() {{ if [[ ${{1-}} == __complete ]]; then command toolr \"$@\"; \
             else print -r -- \"LINE:$*\"; fi; }}\n\
             _toolr_test_dump() {{ print -r -- \"BUF:$BUFFER\"; BUFFER=; zle accept-line; }}\n\
             zle -N _toolr_test_dump\nbindkey '^Xd' _toolr_test_dump\n{rc}\n",
        ),
    )
    .unwrap();
    let mut cmd = StdCommand::new("zsh");
    cmd.args(["--no-globalrcs", "-i"])
        .current_dir(fx.tmp.path())
        .env("ZDOTDIR", fx.tmp.path())
        .env("HOME", fx.tmp.path())
        .env("PATH", path_with_toolr())
        .env("XDG_CACHE_HOME", common::test_cache_home())
        .env("TERM", "dumb");
    let (out, tty) = common::run_on_pty_with_tty(cmd, input);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        !stdout.contains("command not found"),
        "{input:?}: {stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    (stdout, String::from_utf8_lossy(&tty).into_owned())
}

/// The installed `~/.zfunc/_toolr` is autoloaded on the first Tab of a new shell. That load
/// must complete, not just define `_toolr`, and the sourced mode must still work.
#[test]
fn zsh_first_tab_completes_when_autoloaded_or_sourced() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    for load in [ZshLoad::Autoloaded, ZshLoad::Sourced] {
        let stdout = zsh_tab_with(&fx, load, "", "--mode dry\\ w", "\n");
        assert!(
            stdout
                .lines()
                .any(|l| l.trim_end_matches('\r') == "LINE:ci lint --mode dry walk"),
            "{load:?}: {stdout}"
        );
    }
}

/// A live Tab: zsh hands `_toolr` the word as typed, and quotes the inserted match to fit the
/// quote at the cursor, so the submitted line parses back to the plain value.
#[test]
fn zsh_tab_completes_quoted_and_escaped_partials_end_to_end() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    let sub = format!("--mode $(touch {})", fx.sentinel.display());
    let rows = [
        (r"--mode dry\ w", "--mode dry walk"),
        ("--mode 'dry w", "--mode dry walk"),
        (r#"--mode "dry w"#, "--mode dry walk"),
        ("--mode $'dry w", "--mode dry walk"),
        (r"--mode=dry\ w", "--mode=dry walk"),
        ("--mode='dry w", "--mode=dry walk"),
        ("--mode dr'y w", "--mode dry walk"),
        ("--cfg='files/with s", "--cfg=files/with space.txt"),
        ("'--mode' fa", "--mode fast"),
        (r#"--mode "wo"#, "--mode wow!"),
        (r#"--cfg "files/with s"#, "--cfg files/with space.txt"),
    ];
    for (typed, want) in rows.into_iter().chain([("--mode '$(touch", sub.as_str())]) {
        let stdout = zsh_tab(&fx, typed, "\n");
        let want = format!("LINE:ci lint {want}");
        assert!(
            stdout.lines().any(|l| l.trim_end_matches('\r') == want),
            "{typed:?}: want {want:?}\n{stdout}"
        );
    }
    // `pos` has a positional, so a raw `'--fla'` or `\-\-fla` reads as that positional and the
    // engine would return `alpha`/`beta`, no flags. Dequoted it is a flag word, which only
    // `cur="${(Q)raw}"` gives the engine.
    for typed in ["'--fla'", r#""--fla""#, r"\-\-fla"] {
        let (stdout, _) = zsh_session(
            &fx,
            ZshLoad::Sourced,
            "",
            &format!("toolr ci pos {typed}\t\nexit\n"),
        );
        assert!(
            stdout
                .lines()
                .any(|l| l.trim_end_matches('\r') == "LINE:ci pos --flavour"),
            "{typed:?}: {stdout}"
        );
    }
    // zsh completes the command inside these itself; nothing toolr offers may land or run.
    for typed in [r#"--mode "$(to"#, "--mode `to"] {
        let stdout = zsh_tab(&fx, typed, "\x18d");
        assert!(stdout.contains("BUF:"), "{typed:?}: {stdout}");
        assert!(!stdout.contains("sentinel"), "{typed:?}: {stdout}");
    }
    assert!(!fx.sentinel.exists(), "a typed $(...) ran");
    assert!(!fx.sentinel2.exists(), "a typed backtick ran");
}

#[test]
fn fish_quoted_and_escaped_partials_match_a_choice() {
    if !require("fish") {
        return;
    }
    let fx = fixture();
    for (cur, want) in [
        (r"--mode dry\ r", "dry run"),
        ("--mode 'dry r", "dry run"),
        (r#"--mode "dry r"#, "dry run"),
        ("--mode 'dry r'", "dry run"),
        (r"--mode=dry\ r", "--mode=dry run"),
    ] {
        let stdout = fish_complete(&fx, &format!("toolr ci lint {cur}"));
        assert_eq!(fish_names(&stdout), vec![want], "{cur:?}");
    }
}

/// fish's path completers parse their argument as shell text, so they must get the raw word.
#[test]
fn fish_quoted_and_escaped_path_partials_still_complete() {
    if !require("fish") {
        return;
    }
    let fx = fixture();
    for (cur, want) in [
        (r"--cfg files/with\ s", "files/with space.txt"),
        ("--cfg 'files/with s", "files/with space.txt"),
        (r"--cfg=files/with\ s", "--cfg=files/with space.txt"),
    ] {
        let stdout = fish_complete(&fx, &format!("toolr ci lint {cur}"));
        assert!(fish_names(&stdout).contains(&want), "{cur:?}: {stdout}");
    }
}

/// A lone or empty quote unescapes to an empty word, which offers every choice.
#[test]
fn fish_empty_quoted_partial_offers_every_choice() {
    if !require("fish") {
        return;
    }
    let fx = fixture();
    let mut want = fx.choices();
    want.sort();
    for cur in ["\"", "''", "\"\""] {
        let stdout = fish_complete(&fx, &format!("toolr ci lint --mode {cur}"));
        let mut got: Vec<String> = fish_names(&stdout).iter().map(|n| n.to_string()).collect();
        got.sort();
        assert_eq!(got, want, "{cur:?}");
    }
}

/// An escaped newline unescapes to one word; a command substitution alone would split it in two.
#[test]
fn fish_unescaped_newline_stays_one_word() {
    if !require("fish") {
        return;
    }
    let fx = fixture();
    // The shim writes the count to a file: its stdout is the script's `(…)`, not ours.
    let shim = "function toolr; count $argv > argc.out; command toolr $argv; end";
    fish_complete_with(&fx, shim, r"toolr ci lint --mode x\nal");
    let argc = std::fs::read_to_string(fx.tmp.path().join("argc.out")).unwrap();
    // `__complete`, the cwd, `ci`, `lint`, `--mode` and the word being completed.
    assert_eq!(argc.trim(), "6");
}

/// A typed `$(...)` or backtick, quoted or not, is matched as text and never runs.
#[test]
fn fish_quoted_substitution_partial_is_inert() {
    if !require("fish") {
        return;
    }
    let fx = fixture();
    for cur in ["'$(touch", r#""$(touch"#, r"\$\(touch", "'`touch"] {
        let stdout = fish_complete(&fx, &format!("toolr ci lint --mode {cur}"));
        assert!(stdout.contains("touch"), "{cur:?}: {stdout}");
    }
    assert!(!fx.sentinel.exists(), "a typed $(...) ran");
    assert!(!fx.sentinel2.exists(), "a typed backtick ran");
}

// --- zsh and fish ask for every candidate and match the typed word themselves ---

/// Every fixture choice, in the engine's order.
fn sorted_choices(fx: &Fixture) -> Vec<String> {
    let mut all = fx.choices();
    all.sort();
    all
}

/// The `COMPADD:` line for `flags`, then a `VALUE:` line per value.
fn compadd_lines(flags: &str, values: &[String]) -> Vec<String> {
    std::iter::once(compadd_line(flags))
        .chain(values.iter().map(|v| format!("VALUE:{v}")))
        .collect()
}

/// The line of `script` that runs `toolr __complete`, comments aside.
fn complete_call(script: &str) -> String {
    let calls: Vec<&str> = script
        .lines()
        .filter(|l| !l.trim_start().starts_with('#') && l.contains("toolr __complete"))
        .collect();
    assert_eq!(calls.len(), 1, "{calls:?}");
    calls[0].to_string()
}

#[test]
fn zsh_script_asks_the_engine_for_shell_matching() {
    let call = complete_call(&print_script("zsh"));
    assert!(
        call.contains("TOOLR_COMPLETE_MATCH=shell toolr __complete"),
        "{call}"
    );
    assert!(!complete_call(&print_script("bash")).contains("TOOLR_COMPLETE_MATCH"));
}

/// The engine no longer drops `fast` for `FA`: `compadd` gets every choice and zsh's
/// `matcher-list` decides, whether the partial is separate or attached with `=`.
#[test]
fn zsh_uppercase_partial_reaches_compadd() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    for (word, compset) in [
        (vec!["--mode", "FA"], None),
        (vec!["--mode=FA"], Some("COMPSET:-P 1 *=")),
    ] {
        let mut words = vec!["toolr", "ci", "lint"];
        words.extend(word);
        let lines = zsh_run_with(&fx, "", "", &words);
        let calls: Vec<String> = lines
            .iter()
            .filter(|l| !l.starts_with("DISP:"))
            .cloned()
            .collect();
        let mut want: Vec<String> = compset.map(String::from).into_iter().collect();
        want.extend(compadd_lines(DESCRIBED_FLAGS, &sorted_choices(&fx)));
        assert_eq!(calls, want, "{words:?}");
        // The choices carry no description, so each is displayed as itself.
        assert_eq!(disp_lines(&lines), sorted_choices(&fx), "{words:?}");
    }
}

#[test]
fn zsh_command_slot_offers_all_commands_for_any_partial() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    let groups = zsh_values(&fx, "", &["toolr", "ZZ"]);
    for want in ["ci", "project", "self"] {
        assert!(groups.iter().any(|g| g == want), "{want} not in {groups:?}");
    }
    assert_eq!(
        zsh_values(&fx, "", &["toolr", "ci", "ZZ"]),
        vec!["lint", "pos"]
    );
}

/// Descriptions line up with their values, padded to the widest name.
#[test]
fn zsh_descriptions_follow_their_values() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    let lines = zsh_run_with(&fx, "", "", &["toolr", "ci", ""]);
    assert_eq!(
        lines,
        vec![
            compadd_line(DESCRIBED_FLAGS),
            "VALUE:lint".to_string(),
            "VALUE:pos".to_string(),
            "DISP:lint  -- Lint things.".to_string(),
            "DISP:pos   -- A positional and a flag.".to_string(),
        ]
    );
    let lines = zsh_run_with(&fx, "", "", &["toolr", "ci", "lint", "--mode", ""]);
    // Choices have no description, so the listing shows each value alone, unpadded.
    assert_eq!(disp_lines(&lines), sorted_choices(&fx));
}

/// A tab in a choice never reaches `compadd`: the engine drops the value, and its neighbour
/// keeps its own entry. A tab in a description becomes a space. The manifest is hand-built
/// because the build rejects such a choice from source.
#[test]
fn zsh_value_with_a_tab_never_reaches_compadd() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    common::write_fresh_manifest(
        fx.tmp.path(),
        r#"{
            "schema_version": 1, "static_hash": "h", "third_party_hash": "",
            "groups": [{"name": "probe", "title": "Probe", "description": "", "origin": "static"}],
            "commands": [{
                "name": "go", "group": "probe", "module": "tools.probe", "function": "go",
                "summary": "Go\tfast.", "description": "",
                "arguments": [{
                    "name": "mode", "kind": "positional", "help": "Mode.",
                    "allowed_values": ["a\tb", "plain"]
                }],
                "imports": [], "origin": "static"
            }]
        }"#,
    );
    let lines = zsh_run_with(&fx, "", "", &["toolr", "probe", "go", ""]);
    let values: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("VALUE:"))
        .collect();
    assert_eq!(values, vec!["plain"], "{lines:?}");
    assert!(lines.contains(&"DISP:plain".to_string()), "{lines:?}");
    let lines = zsh_run_with(&fx, "", "", &["toolr", "probe", ""]);
    assert!(lines.contains(&"VALUE:go".to_string()), "{lines:?}");
    assert!(
        lines.contains(&"DISP:go  -- Go fast.".to_string()),
        "{lines:?}"
    );
    assert!(lines.iter().all(|l| !l.contains('\t')), "{lines:?}");
}

/// Names pad by display width, not by character count: a double-width name counts two columns
/// and a combining mark none, so the `--` of every row lands in the same column.
#[test]
fn zsh_display_pads_wide_and_combining_names() {
    if !require("zsh") {
        return;
    }
    let shim = r#"toolr() { printf 'ab\tfirst\n日本語\tsecond\ne\u0301e\tthird\nx\n'; }"#;
    // The widest name is 日本語 at six columns.
    assert_eq!(
        zsh_display_for(shim),
        vec![
            "ab      -- first",
            "日本語  -- second",
            "e\u{301}e      -- third",
            "x",
        ]
    );
}

/// A value holding glob characters finds its own line: the lookup takes it literally, so
/// `a*c` doesn't resolve to `abc` nor `a[b]` to `ab`, which come first in the reply.
#[test]
fn zsh_display_looks_values_up_literally() {
    if !require("zsh") {
        return;
    }
    let shim = r#"toolr() { printf 'abc\tone\na*c\ttwo\nab\tthree\na[b]\tfour\n'; }"#;
    assert_eq!(
        zsh_display_for(shim),
        vec![
            "abc   -- one",
            "a*c   -- two",
            "ab    -- three",
            "a[b]  -- four",
        ]
    );
}

/// Adds a `big` group of 25 commands, `cmd00` to `cmd24`, each with its own summary.
fn big_described_group(fx: &Fixture) {
    described_group(fx, 25);
}

/// Adds a `big` group of `count` commands, `cmd00` onwards, each with its own summary.
fn described_group(fx: &Fixture, count: usize) {
    let commands: String = (0..count)
        .map(|i| {
            format!(
                "@group.command\ndef cmd{i:02}(ctx):\n    \"\"\"Summary number {i:02}.\"\"\"\n\n"
            )
        })
        .collect();
    std::fs::write(
        fx.tmp.path().join("tools/big.py"),
        format!("\"\"\"Big.\"\"\"\ngroup = command_group(\"big\", \"Big\", docstring=__doc__)\n\n{commands}"),
    )
    .unwrap();
}

/// Past 20 matches the compact grid is used, so no `display` array is built. The stub
/// `compadd -O` matches every candidate, so all 25 commands count.
#[test]
fn zsh_large_choice_set_skips_descriptions() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    big_described_group(&fx);
    let many: Vec<String> = (0..25).map(|i| format!("cmd{i:02}")).collect();
    let lines = zsh_run_with(&fx, "", "", &["toolr", "big", "C"]);
    assert_eq!(lines, compadd_lines(GRID_FLAGS, &many));
}

/// A live Tab on a slot of more than 20 described commands: descriptions are listed once the
/// typed word narrows the matches to 20 or fewer, by prefix or through the `matcher-list`, and
/// not while more than 20 match.
#[test]
fn zsh_tab_lists_descriptions_when_the_matches_are_few() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    big_described_group(&fx);
    // Without `LIST_AMBIGUOUS` zsh lists even when it rewrites `CMD0` to `cmd0`.
    let matcher = "zstyle ':completion:*' matcher-list '' 'm:{a-zA-Z}={A-Za-z}'\n\
                   unsetopt LIST_AMBIGUOUS";
    for (rc, typed, want) in [
        ("", "cmd0", Some("03")),
        ("", "cmd1", Some("13")),
        (matcher, "CMD0", Some("03")),
        ("", "cmd", None),
        (matcher, "CMD", None),
    ] {
        let (_, tty) = zsh_session(
            &fx,
            ZshLoad::Sourced,
            rc,
            &format!("toolr big {typed}\t\x18dexit\n"),
        );
        match want {
            Some(n) => {
                assert!(
                    tty.contains(&format!("Summary number {n}.")),
                    "{typed:?}: {tty}"
                );
                assert!(!tty.contains("Summary number 24."), "{typed:?}: {tty}");
            }
            None => {
                assert!(tty.contains("cmd24"), "{typed:?}: {tty}");
                assert!(!tty.contains("Summary number"), "{typed:?}: {tty}");
            }
        }
    }
}

/// The rows of `tty` that hold `needle`.
fn tty_rows<'a>(tty: &'a str, needle: &str) -> Vec<&'a str> {
    tty.lines().filter(|l| l.contains(needle)).collect()
}

/// A live Tab on 20 or fewer described commands lists one row per match, each holding its own
/// name ahead of its own description, also when the `matcher-list` widened the match.
#[test]
fn zsh_tab_lists_each_name_with_its_description() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    big_described_group(&fx);
    let matcher = "zstyle ':completion:*' matcher-list '' 'm:{a-zA-Z}={A-Za-z}'\n\
                   unsetopt LIST_AMBIGUOUS";
    for (rc, typed) in [("", "cmd0"), (matcher, "CMD0")] {
        let (_, tty) = zsh_session(
            &fx,
            ZshLoad::Sourced,
            rc,
            &format!("toolr big {typed}\t\x18dexit\n"),
        );
        for i in 0..10 {
            let rows = tty_rows(&tty, &format!("Summary number 0{i}."));
            assert_eq!(rows.len(), 1, "{typed:?} {i}: {tty}");
            let (name, desc) = (
                rows[0].find(&format!("cmd0{i}")),
                rows[0].find(&format!("Summary number 0{i}.")),
            );
            assert!(
                name.is_some() && name < desc,
                "{typed:?} {i}: {:?}\n{tty}",
                rows[0]
            );
            assert_eq!(
                rows[0].matches("cmd").count(),
                1,
                "{typed:?}: {:?}",
                rows[0]
            );
        }
        assert_eq!(
            tty_rows(&tty, "Summary number").len(),
            10,
            "{typed:?}: {tty}"
        );
    }
}

/// A live Tab on 20 matches shows names with descriptions; on 21 the grid takes over.
#[test]
fn zsh_tab_names_appear_at_exactly_twenty_matches() {
    if !require("zsh") {
        return;
    }
    for (count, listed) in [(20, true), (21, false)] {
        let fx = fixture();
        described_group(&fx, count);
        let (_, tty) = zsh_session(&fx, ZshLoad::Sourced, "", "toolr big cmd\t\x18dexit\n");
        if listed {
            let rows = tty_rows(&tty, "Summary number 00.");
            assert!(
                rows.len() == 1 && rows[0].contains("cmd00"),
                "{count}: {tty}"
            );
            assert_eq!(tty_rows(&tty, "Summary number").len(), 20, "{count}: {tty}");
        } else {
            assert!(tty.contains("cmd20"), "{count}: {tty}");
            assert!(!tty.contains("Summary number"), "{count}: {tty}");
        }
    }
}

/// A live Tab with a `descriptions` format: one neutral `toolr` header over groups, commands,
/// flags and values alike, never a `value` header and never the `values` tag.
#[test]
fn zsh_tab_header_is_neutral_for_every_slot_kind() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    let rc = "zstyle ':completion:*:descriptions' format 'HDR<%d>'\n\
              zstyle ':completion:*:values' format 'VALUES<%d>'\n\
              unsetopt LIST_AMBIGUOUS";
    for typed in [
        "toolr ci ",
        "toolr ci lint --",
        "toolr ci lint --mode ",
        "toolr ",
    ] {
        let (_, tty) = zsh_session(&fx, ZshLoad::Sourced, rc, &format!("{typed}\t\x18dexit\n"));
        assert!(tty.contains("HDR<toolr>"), "{typed:?}: {tty}");
        assert!(!tty.contains("HDR<value>"), "{typed:?}: {tty}");
        assert!(!tty.contains("VALUES<"), "{typed:?}: {tty}");
    }
}

/// A live Tab: with a `matcher-list` set, zsh completes a word the engine would have filtered
/// out; without one, it doesn't.
#[test]
fn zsh_tab_applies_the_matcher_list() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    let rc = "zstyle ':completion:*' matcher-list '' 'm:{a-zA-Z}={A-Za-z}' 'l:|=* r:|=*'";
    for (typed, want) in [
        ("--mode FA", "--mode fast"),
        ("--mode=FA", "--mode=fast"),
        (r"--mode DRY\ W", "--mode dry walk"),
        ("--mode alk", "--mode dry walk"),
        ("--mode=alk", "--mode=dry walk"),
    ] {
        let stdout = zsh_tab_with(&fx, ZshLoad::Sourced, rc, typed, "\n");
        let want = format!("LINE:ci lint {want}");
        assert!(
            stdout.lines().any(|l| l.trim_end_matches('\r') == want),
            "{typed:?}: want {want:?}\n{stdout}"
        );
    }
    let stdout = zsh_tab(&fx, "--mode FA", "\x18d");
    assert!(
        stdout
            .lines()
            .any(|l| l.trim_end_matches('\r') == "BUF:toolr ci lint --mode FA"),
        "{stdout}"
    );
    assert!(!fx.sentinel.exists());
    assert!(!fx.sentinel2.exists());
}

/// A script installed by an older toolr never sets the variable, so the new binary filters.
#[test]
fn old_zsh_script_gets_prefix_filtered_output_from_new_binary() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    let old = print_script("zsh").replace("TOOLR_COMPLETE_MATCH=shell toolr", "toolr");
    assert!(!complete_call(&old).contains("TOOLR_COMPLETE_MATCH"));
    let unset = "unset TOOLR_COMPLETE_MATCH";
    let run = |cur: &str| -> Vec<String> {
        zsh_run_script(
            &fx,
            &old,
            "",
            unset,
            &["toolr", "ci", "lint", "--mode", cur],
        )
        .into_iter()
        .filter_map(|l| l.strip_prefix("VALUE:").map(str::to_string))
        .collect()
    };
    assert_eq!(run("FA"), Vec::<String>::new());
    assert_eq!(run("fa"), vec!["fast".to_string()]);
}

/// A binary that predates the variable prefix-filters; the new script still hands `compadd`
/// what it gets.
#[test]
fn new_zsh_script_degrades_on_a_binary_that_ignores_the_variable() {
    if !require("zsh") {
        return;
    }
    let fx = fixture();
    let shim = r#"toolr() { env -u TOOLR_COMPLETE_MATCH "$(whence -p toolr)" "$@"; }"#;
    let lines = zsh_run_with(&fx, "", shim, &["toolr", "ci", "lint", "--mode", "fa"]);
    assert_eq!(
        lines,
        vec![
            compadd_line(DESCRIBED_FLAGS),
            "VALUE:fast".to_string(),
            "DISP:fast".to_string(),
        ]
    );
    let lines = zsh_run_with(&fx, "", shim, &["toolr", "ci", "lint", "--mode", "FA"]);
    assert_eq!(lines, Vec::<String>::new());
}

#[test]
fn fish_script_asks_the_engine_for_shell_matching() {
    let script = print_script("fish");
    assert!(
        script.contains("set -lx TOOLR_COMPLETE_MATCH shell"),
        "{script}"
    );
    assert!(complete_call(&script).contains("toolr __complete"));
}

/// fish matches case-insensitively and by substring once the engine stops filtering.
#[test]
fn fish_uppercase_and_substring_partials_complete() {
    if !require("fish") {
        return;
    }
    let fx = fixture();
    for (line, want) in [
        ("toolr CI", vec!["ci"]),
        ("toolr ci LI", vec!["lint"]),
        ("toolr ci lint --mode FA", vec!["fast"]),
        ("toolr ci lint --mode alk", vec!["dry walk"]),
        ("toolr ci lint --mode=FA", vec!["--mode=fast"]),
        ("toolr ci lint --MO", vec!["--mode"]),
    ] {
        let stdout = fish_complete(&fx, line);
        assert_eq!(fish_names(&stdout), want, "{line:?}");
    }
    // fish matches the whole `--mode=` word, so `alk` is no substring of it there and fish falls
    // back to subsequence matching, which finds more than `dry walk`.
    let stdout = fish_complete(&fx, "toolr ci lint --mode=alk");
    assert!(fish_names(&stdout).contains(&"--mode=dry walk"), "{stdout}");
    assert!(!fx.sentinel.exists());
    assert!(!fx.sentinel2.exists());
}

/// The variable is local to the completion function, so it never reaches the user's session.
#[test]
fn fish_matching_variable_stays_out_of_the_session() {
    if !require("fish") {
        return;
    }
    let fx = fixture();
    let leak = "complete -C 'toolr ci lint --mode '; set -q TOOLR_COMPLETE_MATCH; and echo LEAKED";
    let stdout = fish_complete_with(&fx, leak, "toolr ");
    assert!(!stdout.contains("LEAKED"), "{stdout}");
    assert!(fish_names(&stdout).contains(&"fast"), "{stdout}");
}
