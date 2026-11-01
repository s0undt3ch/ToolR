//! Completion scripts must treat repo-derived candidate values as data.
//! A `Literal[...]` value such as `$(touch X)` must never execute on TAB.
#![cfg(unix)]

mod common;

use assert_cmd::Command;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tempfile::TempDir;

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
    common::write_project(tmp.path());
    let fx = Fixture {
        tmp,
        sentinel,
        sentinel2,
    };
    let literal = fx
        .choices()
        .iter()
        .map(|c| format!("{c:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let src = format!(
        r#""""CI utilities."""
from typing import Literal

group = command_group("ci", "CI utilities", docstring=__doc__)

@group.command
def lint(ctx, mode: Literal[{literal}] = "fast"):
    """Lint things.

    Args:
        mode: Lint mode.
    """
    pass
"#
    );
    std::fs::write(fx.tmp.path().join("tools/ci.py"), src).unwrap();

    Command::cargo_bin("toolr")
        .unwrap()
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
    let out = Command::cargo_bin("toolr")
        .unwrap()
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
    const DRIVER: &str = r#"
script=$1; cur=$2; shift 2
if [ "$cur" = @QDRY ]; then cur=$(printf '%q' "dry "); fi
TW=(toolr ci lint --mode "$cur")
for t in $TW_TAIL; do TW+=("$t"); done
_init_completion() { words=("${TW[@]}"); cword=4; cur="${words[cword]}"; prev="${words[cword-1]}"; return 0; }
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
        .arg(print_script("bash"))
        .arg(cur)
        .args(fx.choices())
        .current_dir(fx.tmp.path())
        .env("PATH", path_with_toolr())
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
    }
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
