//! Repo-derived text that clap renders (group titles, summaries, argument
//! help, choices) must not carry terminal control sequences, even when it
//! arrives through a third-party fragment that the parser never vetted.

mod common;

use common::Project;

const OSC: &str = "\x1b]0;x\x07";

/// Every manifest string a clap renderer shows carries the OSC payload.
const FRAGMENT: &str = r#"{"toolr_schema_version":2,"package":"evil_plugin",
    "groups":[{"name":"evil","title":"GroupTitle\u001b]0;x\u0007","description":"GroupDesc\u001b]0;x\u0007",
        "origin":"third_party"}],
    "commands":[{"name":"run","group":"evil","module":"evil_plugin.commands",
        "function":"run_fn","summary":"CmdSummary\u001b]0;x\u0007",
        "description":"CmdDesc\u001b]0;x\u0007\n\nSecond paragraph",
        "arguments":[{"name":"mode","kind":"optional","default":"ok",
            "help":"ArgHelp\u001b]0;x\u0007",
            "allowed_values":["ok","bad\u001b[2J"]}],
        "origin":"third_party"},
        {"name":"code","group":"evil","module":"evil_plugin.commands",
        "function":"code_fn","summary":"Code",
        "description":"Intro\n\n```\n    indented  two\n```",
        "arguments":[],"origin":"third_party"},
        {"name":"level","group":"evil","module":"evil_plugin.commands",
        "function":"level_fn","summary":"Level",
        "description":"",
        "arguments":[{"name":"level","kind":"optional","default":"a\u001b[2Jb",
            "help":"Level","allowed_values":["a","b"]}],
        "origin":"third_party"}]}"#;

/// Dirty group, command and argument names, a dirty group reference, and
/// one clean command that must survive the filter.
const DIRTY_NAMES_FRAGMENT: &str = r#"{"toolr_schema_version":2,"package":"dirty_plugin",
    "groups":[{"name":"bad\u001b]0;X\u0007","title":"Bad","description":"","origin":"third_party"},
        {"name":"tidy","title":"Tidy","description":"","origin":"third_party"}],
    "commands":[{"name":"inner","group":"bad\u001b]0;X\u0007","module":"dirty_plugin.commands",
        "function":"inner_fn","summary":"Inner","description":"","arguments":[],"origin":"third_party"},
        {"name":"b\u001b]0;X\u0007ad","group":"tidy","module":"dirty_plugin.commands",
        "function":"bad_fn","summary":"BadCmd","description":"","arguments":[],"origin":"third_party"},
        {"name":"ghost","group":"gh\u001b]0;X\u0007ost","module":"dirty_plugin.commands",
        "function":"ghost_fn","summary":"Ghost","description":"","arguments":[],"origin":"third_party"},
        {"name":"argy","group":"tidy","module":"dirty_plugin.commands",
        "function":"argy_fn","summary":"Argy","description":"",
        "arguments":[{"name":"m\u001b]0;X\u0007ode","kind":"optional","default":"a","help":"Mode"},
            {"name":"level","kind":"optional","default":"a","help":"Level"}],
        "origin":"third_party"},
        {"name":"ok","group":"tidy","module":"dirty_plugin.commands",
        "function":"ok_fn","summary":"Fine","description":"","arguments":[],"origin":"third_party"}]}"#;

const DIRTY_OSC: &str = "\x1b]0;X\x07";

fn project() -> Project {
    project_with_fragment(FRAGMENT)
}

fn project_with_fragment(fragment: &str) -> Project {
    let p = Project::new();
    p.add_plugin("evil_plugin", fragment);
    p
}

fn run(p: &Project, args: &[&str]) -> (bool, String) {
    let out = p.toolr(args).env("CLICOLOR_FORCE", "1").output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

fn assert_clean(text: &str) {
    assert!(!text.contains(OSC), "OSC payload leaked:\n{text:?}");
    assert!(!text.contains("\x1b]"), "OSC introducer leaked:\n{text:?}");
    assert!(!text.contains('\x07'), "BEL leaked:\n{text:?}");
    assert!(!text.contains("[2J"), "choice payload leaked:\n{text:?}");
}

#[test]
fn help_at_every_level_carries_no_payload() {
    let p = project();
    for args in [
        &["--help"][..],
        &["evil", "--help"],
        &["evil", "-h"],
        &["evil", "run", "--help"],
        &["evil", "run", "-h"],
    ] {
        let (ok, text) = run(&p, args);
        assert!(ok, "{args:?}: {text}");
        assert_clean(&text);
    }
    let (_, text) = run(&p, &["evil", "run", "--help"]);
    for kept in ["CmdDesc", "ArgHelp", "Second paragraph"] {
        assert!(text.contains(kept), "{kept} missing:\n{text}");
    }
    assert!(text.contains("ok"), "valid choice missing:\n{text}");
}

#[test]
fn invalid_value_error_lists_no_control_char_choice() {
    let p = project();
    let (ok, text) = run(&p, &["evil", "run", "--mode", "nope"]);
    assert!(!ok);
    assert_clean(&text);
    assert!(text.contains("possible values"), "{text}");
}

#[test]
fn control_char_choice_is_rejected_as_a_value() {
    let p = project();
    let (ok, text) = run(&p, &["evil", "run", "--mode", "bad\x1b[2J"]);
    assert!(!ok, "a dropped choice must not be accepted");
    assert!(text.contains("invalid value"), "{text}");
}

#[test]
fn clean_description_lines_keep_indentation_and_inner_spaces() {
    let p = project();
    let (ok, text) = run(&p, &["evil", "code", "--help"]);
    assert!(ok, "{text}");
    assert!(text.contains("    indented  two"), "{text:?}");
}

#[test]
fn control_char_default_is_dropped_and_help_still_works() {
    let p = project();
    let (ok, text) = run(&p, &["evil", "level", "--help"]);
    assert!(ok, "{text}");
    assert_clean(&text);
    let (ok, text) = run(&p, &["evil", "level", "--level", "a", "--help"]);
    assert!(ok, "{text}");
}

fn assert_no_dirty_names(text: &str) {
    assert!(!text.contains(DIRTY_OSC), "dirty name leaked:\n{text:?}");
    assert!(!text.contains('\x1b'), "ESC leaked:\n{text:?}");
    assert!(!text.contains('\x07'), "BEL leaked:\n{text:?}");
}

#[test]
fn fragment_with_dirty_names_never_reaches_clap() {
    let p = project_with_fragment(DIRTY_NAMES_FRAGMENT);
    for args in [
        &["--help"][..],
        &["tidy", "--help"],
        &["tidy", "ok", "--help"],
    ] {
        let (ok, text) = run(&p, args);
        assert!(ok, "{args:?}: {text}");
        assert_no_dirty_names(&text);
    }
    let (_, text) = run(&p, &["--help"]);
    assert!(text.contains("tidy"), "{text}");
    assert!(
        text.contains("dropped"),
        "no warning for dropped names:\n{text}"
    );
    let (_, text) = run(&p, &["tidy", "--help"]);
    assert!(text.contains("ok"), "{text}");
    assert!(!text.contains("BadCmd") && !text.contains("Argy"), "{text}");
    let (ok, text) = run(&p, &["greet", "hi", "--help"]);
    assert!(ok, "{text}");
    assert!(text.contains("Say hi"), "{text}");
}

/// A committed manifest edited by hand keeps its freshness hashes, so
/// toolr trusts it without a rebuild.
fn project_with_hand_edited_manifest() -> Project {
    let p = Project::new();
    p.toolr(&["__build-static-manifest"]).assert().success();
    // The first run stamps the third-party hash, so the edit below stays fresh.
    let (ok, text) = run(&p, &["--help"]);
    assert!(ok, "{text}");
    let path = p.tools().join(".toolr-manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let cmd = |name: &str, group: &str, arg: &str| {
        serde_json::json!({
            "name": name, "group": group, "module": "tools.greet", "function": "hi",
            "summary": "Edited", "description": "",
            "arguments": [{"name": arg, "kind": "optional", "default": "a", "help": "H"}],
            "imports": [], "origin": "static"
        })
    };
    manifest["groups"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "name": "ev\u{1b}]0;X\u{7}il", "title": "Evil", "description": "", "origin": "static"
        }));
    let commands = manifest["commands"].as_array_mut().unwrap();
    commands.push(cmd("x\u{1b}]0;X\u{7}y", "greet", "mode"));
    commands.push(cmd("inner", "ev\u{1b}]0;X\u{7}il", "mode"));
    commands.push(cmd("flagged", "greet", "m\u{1b}]0;X\u{7}ode"));
    std::fs::write(&path, serde_json::to_string(&manifest).unwrap()).unwrap();
    p
}

#[test]
fn hand_edited_fresh_manifest_with_dirty_names_never_reaches_clap() {
    let p = project_with_hand_edited_manifest();
    for args in [
        &["--help"][..],
        &["greet", "--help"],
        &["greet", "hi", "--help"],
    ] {
        let (ok, text) = run(&p, args);
        assert!(ok, "{args:?}: {text}");
        assert_no_dirty_names(&text);
    }
    let (_, text) = run(&p, &["greet", "hi", "--help"]);
    assert!(text.contains("Say hi"), "{text}");
    let (_, text) = run(&p, &["greet", "--help"]);
    assert!(text.contains("hi") && !text.contains("Edited"), "{text}");
}

/// Arguments whose names or flags carry control characters, each on its
/// own command beside a clean one.
const DIRTY_ARGS_FRAGMENT: &str = r#"{"toolr_schema_version":2,"package":"dirty_args",
    "groups":[{"name":"tidy","title":"Tidy","description":"","origin":"third_party"}],
    "commands":[{"name":"colour","group":"tidy","module":"dirty_args.commands",
        "function":"colour_fn","summary":"Colour","description":"",
        "arguments":[{"name":"color","kind":"optional","default":"a","help":"C",
            "long_flag":"--co\u001b]0;X\u0007lor"}],"origin":"third_party"},
        {"name":"pos","group":"tidy","module":"dirty_args.commands",
        "function":"pos_fn","summary":"Pos","description":"",
        "arguments":[{"name":"f\u001b]0;X\u0007x","kind":"positional","help":"F"},
            {"name":"mode","kind":"positional","help":"M","allowed_values":["fast","slow"]}],
        "origin":"third_party"},
        {"name":"req","group":"tidy","module":"dirty_args.commands",
        "function":"req_fn","summary":"Req","description":"",
        "arguments":[{"name":"a","kind":"optional","default":"x","help":"A",
            "metadata":{"requires":["b\u001bx"]}},
            {"name":"b\u001bx","kind":"optional","default":"y","help":"B"}],
        "origin":"third_party"},
        {"name":"ok","group":"tidy","module":"dirty_args.commands",
        "function":"ok_fn","summary":"Fine","description":"",
        "arguments":[{"name":"level","kind":"optional","default":"a","help":"L"}],
        "origin":"third_party"}]}"#;

fn complete(p: &Project, words: &[&str]) -> String {
    let cwd = p.tmp.path().to_string_lossy().to_string();
    let mut args = vec!["__complete", cwd.as_str()];
    args.extend_from_slice(words);
    let (ok, text) = run(p, &args);
    assert!(ok, "{words:?}: {text}");
    text
}

/// Tab completion reads the committed manifest without globbing the venv,
/// so one dispatch run is needed to merge the fragment into it first.
fn dirty_args_project() -> Project {
    let p = project_with_fragment(DIRTY_ARGS_FRAGMENT);
    let (ok, text) = run(&p, &["--help"]);
    assert!(ok, "{text}");
    p
}

#[test]
fn command_with_a_dirty_argument_is_dropped_whole() {
    let p = dirty_args_project();
    for args in [
        &["--help"][..],
        &["tidy", "--help"],
        &["tidy", "ok", "--help"],
    ] {
        let (ok, text) = run(&p, args);
        assert!(ok, "{args:?}: {text}");
        assert_no_dirty_names(&text);
        assert!(!text.contains("panicked"), "{text}");
    }
    let (_, text) = run(&p, &["tidy", "--help"]);
    assert!(text.contains("ok"), "{text}");
    for gone in ["colour", "pos", "req"] {
        let (ok, text) = run(&p, &["tidy", gone, "--help"]);
        assert!(!ok, "{gone} should be dropped:\n{text}");
        // clap's own error styling is ESC-coloured; only the payloads matter.
        assert!(
            !text.contains("\x1b]") && !text.contains("\x1bx"),
            "{text:?}"
        );
        assert!(!text.contains('\x07'), "{text:?}");
        assert!(!text.contains("panicked"), "{text}");
    }
}

#[test]
fn completion_agrees_with_clap_on_dirty_arguments() {
    let p = dirty_args_project();
    let commands = complete(&p, &["tidy", ""]);
    assert!(commands.contains("ok"), "{commands}");
    for gone in ["colour", "pos", "req"] {
        assert!(!commands.contains(gone), "{gone} offered:\n{commands}");
    }
    let flags = complete(&p, &["tidy", "colour", "--"]);
    assert!(!flags.contains("--color"), "dirty flag offered:\n{flags}");
    let values = complete(&p, &["tidy", "pos", "anything", ""]);
    assert!(
        !values.contains("fast"),
        "dropped positional slot used:\n{values}"
    );
    let flags = complete(&p, &["tidy", "ok", "--"]);
    assert!(flags.contains("--level"), "{flags}");
}
