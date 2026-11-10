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

#[test]
fn scripts_handle_the_exit_codes_dispatch_emits() {
    let dispatch = repo_file("crates/toolr/src/dispatch.rs");
    let paths = dispatch_const(&dispatch, "COMPLETE_PATHS");
    let dirs = dispatch_const(&dispatch, "COMPLETE_DIRECTORIES");
    let cases = [
        ("bash.sh", format!("{paths} | {dirs})")),
        ("zsh.zsh", format!("{paths}) ")),
        ("zsh.zsh", format!("{dirs}) ")),
        ("fish.fish", format!("case {paths}")),
        ("fish.fish", format!("case {dirs}")),
        (
            "powershell.ps1",
            format!("-eq {paths} -or $reply.Code -eq {dirs}"),
        ),
    ];
    for (file, expected) in cases {
        assert!(
            script(file).contains(&expected),
            "{file} does not handle exit code `{expected}`; dispatch.rs has COMPLETE_PATHS={paths}, \
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
