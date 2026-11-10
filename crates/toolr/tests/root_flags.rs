//! toolr's flags (`-d`, `-q`, the timestamp and timeout flags) belong to toolr: they are
//! accepted only before the first subcommand, and a misplaced one gets a hint saying where it
//! goes. Everything here stops at parse time or at help, so no Python runs.

mod common;

use std::process::Output;

use tempfile::TempDir;

const MANIFEST: &str = r#"{
    "schema_version": 1,
    "static_hash": "h",
    "third_party_hash": "",
    "groups": [
        {"name": "probe", "title": "Probe", "description": "", "origin": "static"}
    ],
    "commands": [
        {
            "name": "own",
            "group": "probe",
            "module": "tools.probe",
            "function": "own",
            "summary": "Declares names the root flags also use.",
            "description": "",
            "arguments": [
                {
                    "name": "debug",
                    "kind": "flag",
                    "help": "the command's own debug.",
                    "default": "false",
                    "type_annotation": "bool",
                    "resolved_type": {"kind": "bool"},
                    "allowed_values": [],
                    "metadata": {"conflicts_with": ["fast"]}
                },
                {
                    "name": "fast",
                    "kind": "flag",
                    "help": "go fast, short -q.",
                    "default": "false",
                    "type_annotation": "bool",
                    "resolved_type": {"kind": "bool"},
                    "allowed_values": [],
                    "metadata": {"aliases": ["-q"], "conflicts_with": ["debug"]}
                }
            ],
            "imports": [],
            "origin": "static"
        },
        {
            "name": "named",
            "group": "probe",
            "module": "tools.probe",
            "function": "named",
            "summary": "Parameters named like the root flags.",
            "description": "",
            "arguments": [
                {"name": "quiet", "kind": "optional", "help": "", "default": "\"x\"", "type_annotation": "str", "resolved_type": {"kind": "str"}, "allowed_values": [], "metadata": {}},
                {"name": "debug", "kind": "optional", "help": "", "default": "\"x\"", "type_annotation": "str", "resolved_type": {"kind": "str"}, "allowed_values": [], "metadata": {}},
                {"name": "timestamps", "kind": "optional", "help": "", "default": "\"x\"", "type_annotation": "str", "resolved_type": {"kind": "str"}, "allowed_values": [], "metadata": {}},
                {"name": "timeout_secs", "kind": "optional", "help": "", "default": "1", "type_annotation": "int", "resolved_type": {"kind": "int"}, "allowed_values": [], "metadata": {}}
            ],
            "imports": [],
            "origin": "static"
        },
        {
            "name": "plain",
            "group": "probe",
            "module": "tools.probe",
            "function": "plain",
            "summary": "Declares nothing.",
            "description": "",
            "arguments": [],
            "imports": [],
            "origin": "static"
        }
    ]
}"#;

fn fixture() -> TempDir {
    let tmp = TempDir::new().unwrap();
    common::write_fresh_manifest(tmp.path(), MANIFEST);
    tmp
}

fn run(tmp: &TempDir, args: &[&str]) -> Output {
    common::toolr()
        .current_dir(tmp.path())
        .args(args)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn root_flags_work_before_the_command() {
    let tmp = fixture();
    for flag in [
        &["-d"][..],
        &["--debug"],
        &["-q"],
        &["--timestamps"],
        &["--no-timestamps"],
        &["--timeout-secs", "5"],
        &["--no-output-timeout-secs=5"],
    ] {
        let mut args = flag.to_vec();
        args.extend(["probe", "plain", "--help"]);
        let out = run(&tmp, &args);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
    }
}

#[test]
fn a_misplaced_root_flag_gets_a_hint_with_the_flag_moved_to_the_front() {
    let tmp = fixture();
    // The hint text is pinned by the unit table in `main.rs`; these are the end-to-end cases.
    for (args, example) in [
        (&["probe", "plain", "-d"][..], "toolr -d probe plain ..."),
        (
            &["project", "venv", "sync", "-q", "--if-project"],
            "toolr -q project venv sync ...",
        ),
    ] {
        let out = run(&tmp, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let err = stderr(&out);
        assert!(err.contains("is a toolr option"), "{args:?}: {err}");
        assert!(
            err.trim_end().ends_with(&format!("`{example}`")),
            "{args:?}: {err}"
        );
    }
}

#[test]
fn other_unknown_arguments_get_no_hint() {
    let tmp = fixture();
    let out = run(&tmp, &["probe", "plain", "--nope"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(!stderr(&out).contains("toolr option"));
}

#[test]
fn a_command_that_spells_a_root_flag_itself_keeps_it_and_gets_no_hint() {
    let tmp = fixture();
    // `--debug` and `-q` are the command's own here, so its own conflict rule decides.
    let out = run(&tmp, &["probe", "own", "--debug", "-q"]);
    assert_eq!(out.status.code(), Some(2));
    let err = stderr(&out);
    assert!(err.contains("cannot be used with"), "{err}");
    assert!(!err.contains("toolr option"), "{err}");
}

#[test]
fn only_the_root_help_lists_the_toolr_options() {
    let tmp = fixture();
    let root = stdout(&run(&tmp, &["--help"]));
    assert_eq!(root.matches("ToolR Options:").count(), 1, "{root}");
    // The placement rule sits under the heading, ahead of the first flag, in both help modes.
    // Whitespace is collapsed because the note wraps on a narrow terminal.
    for help in [root.clone(), stdout(&run(&tmp, &["-h"]))] {
        let flat = help.split_whitespace().collect::<Vec<_>>().join(" ");
        let section = &flat[flat.find("ToolR Options:").expect("heading")..];
        let note = section
            .find("toolr's options go before the command: toolr --debug <command> ...")
            .expect("note");
        assert!(
            note < section.find("-d, --debug").expect("flag row"),
            "{help}"
        );
    }
    for flag in [
        "--debug",
        "--quiet",
        "--timeout-secs",
        "--no-output-timeout-secs",
    ] {
        assert!(root.contains(flag), "{root}");
    }
    for args in [
        &["probe", "--help"][..],
        &["probe", "plain", "--help"],
        &["project", "venv", "sync", "--help"],
        &["self", "--help"],
    ] {
        let text = stdout(&run(&tmp, args));
        assert!(!text.contains("ToolR Options"), "{args:?}\n{text}");
        assert!(!text.contains("--quiet"), "{args:?}\n{text}");
    }
}

/// A parameter named like a root flag is an ordinary argument: parsing, `--help` and building
/// the runner spec (which stops at the missing venv) never mix it up with toolr's own flag.
#[test]
fn parameters_named_like_root_flags_are_ordinary_arguments() {
    let tmp = fixture();
    let own = [
        "--quiet",
        "v",
        "--debug",
        "v",
        "--timestamps",
        "v",
        "--timeout-secs",
        "9",
    ];
    for prefix in [&[][..], &["-q"], &["-d"], &["--timestamps"]] {
        let mut help: Vec<&str> = prefix.to_vec();
        help.extend(["probe", "named", "--help"]);
        let out = run(&tmp, &help);
        assert!(out.status.success(), "{help:?}: {}", stderr(&out));

        let mut args: Vec<&str> = prefix.to_vec();
        args.extend(["probe", "named"]);
        args.extend(own);
        let out = run(&tmp, &args);
        let err = stderr(&out);
        assert!(
            err.contains("Python interpreter not found") || out.status.success(),
            "{args:?}: {err}"
        );
        assert!(!err.contains("panicked"), "{args:?}: {err}");
    }
}

/// `venv run` takes its command with hyphen values allowed, so a toolr flag before the
/// command would be run as the command; it gets the hint instead.
#[test]
fn venv_run_does_not_exec_a_misplaced_root_flag() {
    let tmp = fixture();
    for (args, example) in [
        (
            &["project", "venv", "run", "--quiet", "pytest"][..],
            "toolr --quiet project venv run ...",
        ),
        (
            &["project", "venv", "run", "-q", "pytest"],
            "toolr -q project venv run ...",
        ),
        (
            &["project", "venv", "run", "-d", "python", "x.py"],
            "toolr -d project venv run ...",
        ),
        (
            &["project", "venv", "run", "--timeout-secs", "5", "pytest"],
            "toolr --timeout-secs 5 project venv run ...",
        ),
        (
            &["project", "venv", "run", "--no-sync", "-qd", "pytest"],
            "toolr -q project venv run ...",
        ),
    ] {
        let out = run(&tmp, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let err = stderr(&out);
        assert!(err.contains("is a toolr option"), "{args:?}: {err}");
        assert!(
            err.trim_end().ends_with(&format!("`{example}`")),
            "{args:?}: {err}"
        );
    }
    // `venv run -- -q` is hinted too: clap strips the `--`, so only the first token can be
    // checked. Later tokens (`python -q`) are the command's own.
    let out = run(&tmp, &["project", "venv", "run", "--", "-q"]);
    assert!(
        stderr(&out).contains("is a toolr option"),
        "{}",
        stderr(&out)
    );
    let out = run(&tmp, &["project", "venv", "run", "--", "python", "-q"]);
    assert!(!stderr(&out).contains("toolr option"), "{}", stderr(&out));
}

/// What the user typed is never echoed raw: a control-character value stays out of the line.
#[test]
fn the_hint_never_echoes_control_characters() {
    let tmp = fixture();
    let out = run(
        &tmp,
        &[
            "project",
            "venv",
            "run",
            "--timeout-secs=\u{1b}[31mx",
            "pytest",
        ],
    );
    let err = stderr(&out);
    assert!(err.contains("is a toolr option"), "{err}");
    assert!(!err.contains('\u{1b}'), "{err:?}");
    assert!(err.contains("--timeout-secs <SECONDS>"), "{err}");
}
