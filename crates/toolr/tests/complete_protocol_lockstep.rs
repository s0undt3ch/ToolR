//! The `__complete` exit codes and the match env var are a contract shared by the binary and
//! all four embedded scripts. This reads them as text, so it runs with no shell installed.

use std::fs;
use std::path::PathBuf;

fn repo_file(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn script(name: &str) -> String {
    repo_file(&format!("crates/toolr-core/src/complete/scripts/{name}"))
}

/// The `u8` literal of `const <name>: u8 = N;` in `dispatch.rs`.
fn dispatch_const(source: &str, name: &str) -> String {
    let needle = format!("const {name}: u8 = ");
    let rest = source
        .split_once(&needle)
        .unwrap_or_else(|| panic!("no `{needle}` in dispatch.rs"))
        .1;
    rest.split_once(';').expect("terminating `;`").0.to_string()
}

/// Whether some line of `text`, trimmed, satisfies `pred`.
fn has_line(text: &str, pred: impl Fn(&str) -> bool) -> bool {
    text.lines().any(|l| pred(l.trim()))
}

#[test]
fn scripts_handle_the_exit_codes_dispatch_emits() {
    let dispatch = repo_file("crates/toolr/src/dispatch.rs");
    let paths = dispatch_const(&dispatch, "COMPLETE_PATHS");
    let dirs = dispatch_const(&dispatch, "COMPLETE_DIRECTORIES");
    let bash = script("bash.sh");
    let zsh = script("zsh.zsh");
    let fish = script("fish.fish");
    let pwsh = script("powershell.ps1");
    let checks = [
        (
            "bash.sh",
            has_line(&bash, |l| l == format!("{paths} | {dirs})")),
        ),
        (
            "zsh.zsh",
            has_line(&zsh, |l| l.starts_with(&format!("{paths}) "))),
        ),
        (
            "zsh.zsh",
            has_line(&zsh, |l| l.starts_with(&format!("{dirs}) "))),
        ),
        (
            "fish.fish",
            has_line(&fish, |l| l == format!("case {paths}")),
        ),
        (
            "fish.fish",
            has_line(&fish, |l| l == format!("case {dirs}")),
        ),
        (
            "powershell.ps1",
            has_line(&pwsh, |l| {
                l.contains(&format!(
                    "$reply.Code -eq {paths} -or $reply.Code -eq {dirs}"
                ))
            }),
        ),
    ];
    for (file, ok) in checks {
        assert!(
            ok,
            "{file} does not handle the exit codes; dispatch.rs has COMPLETE_PATHS={paths}, \
             COMPLETE_DIRECTORIES={dirs}"
        );
    }
}

#[test]
fn match_env_var_name_matches_the_scripts_that_set_it() {
    let dispatch = repo_file("crates/toolr/src/dispatch.rs");
    let name = dispatch
        .split_once("const COMPLETE_MATCH_VAR: &str = \"")
        .expect("COMPLETE_MATCH_VAR")
        .1
        .split_once('"')
        .expect("closing quote")
        .0;
    assert!(script("zsh.zsh").contains(&format!("{name}=shell toolr __complete")));
    assert!(script("fish.fish").contains(&format!("set -lx {name} shell")));
    assert!(!script("bash.sh").contains(name), "bash filters itself");
}
