mod common;

use tempfile::TempDir;

/// Build a tmpdir containing a tools/ directory with one ci.py file and
/// a freshly-built manifest committed alongside it. This mirrors the
/// happy path: the cached manifest's static_hash matches the live tree.
fn fixture() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let tools = common::write_project(tmp.path());
    std::fs::write(
        tools.join("ci.py"),
        r#""""CI utilities."""
from typing import Literal

group = command_group("ci", "CI utilities", docstring=__doc__)

@group.command
def hello(ctx, name="world"):
    """Say hello.

    Args:
        name: Who to greet.
    """
    pass

@group.command
def deploy(ctx, env: Literal["staging", "production"]):
    """Deploy something."""
    pass
"#,
    )
    .unwrap();

    // Build the manifest in-process so the static_hash matches.
    common::toolr()
        .current_dir(tmp.path())
        .arg("__build-static-manifest")
        .assert()
        .success();

    tmp
}

/// Raw `__complete` stdout: one `value<TAB>description` line per
/// candidate (description may be empty).
fn complete_raw(tmp: &TempDir, args: &[&str]) -> String {
    let cwd = tmp.path().to_path_buf();
    let mut full: Vec<String> = vec!["__complete".into(), cwd.to_string_lossy().to_string()];
    for a in args {
        full.push((*a).to_string());
    }
    let output = common::toolr()
        .current_dir(tmp.path())
        .args(&full)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected __complete to succeed, got status {:?}, stderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Just the candidate values, one per line — for tests that don't care
/// about descriptions.
fn complete(tmp: &TempDir, args: &[&str]) -> String {
    complete_raw(tmp, args)
        .lines()
        .map(|line| line.split('\t').next().unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn description_newline_does_not_forge_candidates() {
    let tmp = TempDir::new().unwrap();
    let tools = common::write_project(tmp.path());
    std::fs::write(
        tools.join("ci.py"),
        "\"\"\"CI.\"\"\"\nfrom toolr import command_group\n\ngroup = command_group(\"ci\", \"x\\nevil\\tdesc\\x1b[2J\", \"CI.\")\n",
    )
    .unwrap();
    common::toolr()
        .current_dir(tmp.path())
        .arg("__build-static-manifest")
        .assert()
        .success();
    let out = complete_raw(&tmp, &["c"]);
    assert_eq!(out.lines().count(), 1, "output: {out:?}");
    assert!(!out.contains('\x1b'), "output: {out:?}");
}

#[test]
fn completes_groups_at_top_level() {
    let tmp = fixture();
    let stdout = complete(&tmp, &[""]);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines.contains(&"ci"), "missing ci in {stdout}");
}

#[test]
fn completions_carry_a_description_after_the_tab() {
    let tmp = fixture();
    let stdout = complete_raw(&tmp, &[""]);
    assert!(
        stdout.lines().any(|line| line == "ci\tCI utilities"),
        "expected `ci` described by its group title, got: {stdout}"
    );

    let stdout = complete_raw(&tmp, &["ci", "h"]);
    assert!(
        stdout.lines().any(|line| line == "hello\tSay hello."),
        "expected `hello` described by its command summary, got: {stdout}"
    );
}

#[test]
fn completes_commands_under_a_group() {
    let tmp = fixture();
    let stdout = complete(&tmp, &["ci", ""]);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines.contains(&"hello"), "missing hello in {stdout}");
    assert!(lines.contains(&"deploy"), "missing deploy in {stdout}");
}

#[test]
fn completes_command_prefixes() {
    let tmp = fixture();
    let stdout = complete(&tmp, &["ci", "h"]);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines, vec!["hello"]);
}

#[test]
fn completes_literal_flag_values() {
    let tmp = fixture();
    let stdout = complete(&tmp, &["ci", "deploy", "--env", ""]);
    let mut lines: Vec<&str> = stdout.lines().collect();
    lines.sort();
    assert_eq!(lines, vec!["production", "staging"]);
}

#[test]
fn returns_no_completions_for_unknown_group() {
    let tmp = fixture();
    let stdout = complete(&tmp, &["unknown", ""]);
    assert!(stdout.trim().is_empty());
}

#[test]
fn reparses_when_tools_change_after_manifest_was_written() {
    let tmp = fixture();
    // Add a new command after the cached manifest was built.
    std::fs::write(
        tmp.path().join("tools/extra.py"),
        r#"group = command_group("extra", "Extra utilities", "Extra utilities.")

@group.command
def shiny(ctx):
    """Shiny new command."""
    pass
"#,
    )
    .unwrap();

    let stdout = complete(&tmp, &[""]);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.contains(&"extra"),
        "missing freshly-added group in {stdout}"
    );
}

#[test]
fn completes_builtins_when_no_tools_dir_anywhere() {
    let tmp = TempDir::new().unwrap();
    let cwd = tmp.path().to_path_buf();
    // Only an ancestor `tools/pyproject.toml` makes a project, so a bare
    // host `tools/` (the Windows runners' `C:\tools\`) is harmless. Skip
    // when the tempdir sits inside a real project.
    let mut walker = cwd.canonicalize().unwrap_or_else(|_| cwd.clone());
    let ancestor_has_tools = loop {
        if walker.join("tools").join("pyproject.toml").is_file() {
            break true;
        }
        if !walker.pop() {
            break false;
        }
    };
    if ancestor_has_tools {
        eprintln!(
            "skipping: an ancestor of {} has a tools/pyproject.toml; \
             this host violates the test precondition.",
            cwd.display(),
        );
        return;
    }
    let output = common::toolr()
        .current_dir(&cwd)
        .args(["__complete", &cwd.to_string_lossy(), ""])
        .output()
        .unwrap();
    // Built-in `self` / `project` subtree doesn't depend on a project
    // root, so completion must still offer them outside any toolr project.
    // Only user-defined commands need a tools/ ancestor.
    assert!(
        output.status.success(),
        "expected success, got {:?}, stderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let values: Vec<&str> = stdout
        .lines()
        .map(|line| line.split('\t').next().unwrap_or(line))
        .collect();
    for expected in ["self", "project"] {
        assert!(
            values.contains(&expected),
            "missing built-in {expected} outside a toolr project; got: {stdout}",
        );
    }
    assert!(output.stderr.is_empty(), "expected silent stderr");
}

#[test]
fn completes_root_flags_on_double_dash_prefix() {
    // Regression: `toolr --<TAB>` returned nothing because the engine
    // landed on the top-level group slot and never offered flags. The
    // binary's root options must surface alongside the engine-level
    // `--help`.
    let tmp = fixture();
    let stdout = complete(&tmp, &["--"]);
    let mut lines: Vec<&str> = stdout.lines().collect();
    lines.sort();
    assert_eq!(
        lines,
        vec![
            "--debug",
            "--help",
            "--no-output-timeout-secs",
            "--no-timestamps",
            "--quiet",
            "--timeout-secs",
            "--timestamps",
        ]
    );
}

#[test]
fn completes_root_flag_prefix_filters_to_matching_flags() {
    // Narrower prefix → only matching long flags.
    let tmp = fixture();
    let stdout = complete(&tmp, &["--no"]);
    let mut lines: Vec<&str> = stdout.lines().collect();
    lines.sort();
    assert_eq!(lines, vec!["--no-output-timeout-secs", "--no-timestamps"]);
}

#[test]
fn completes_help_at_group_node() {
    // `toolr ci --<TAB>` sits on a group; clap injects `--help` on
    // every subcommand, so completion must follow suit.
    let tmp = fixture();
    let stdout = complete(&tmp, &["ci", "--"]);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.contains(&"--help"),
        "expected --help under group node, got: {stdout}"
    );
}

#[test]
fn completes_help_alongside_leaf_flags() {
    // `toolr ci hello --<TAB>` should offer `--help` alongside the
    // leaf's own flags. `hello` has `name="world"` which the parser
    // surfaces as the `--name` optional flag.
    let tmp = fixture();
    let stdout = complete(&tmp, &["ci", "hello", "--"]);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.contains(&"--help"),
        "expected --help at leaf node, got: {stdout}"
    );
    assert!(
        lines.contains(&"--name"),
        "expected --name preserved at leaf node, got: {stdout}"
    );
}

#[test]
fn self_completion_print_emits_bash_script() {
    let tmp = TempDir::new().unwrap();
    let output = common::toolr()
        .current_dir(tmp.path())
        .args(["self", "completion", "print", "bash"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("toolr __complete"));
    assert!(stdout.contains("complete -F _toolr_complete toolr"));
}

/// F-17: TAB on a tree with a symlinked module stays silent. The resolve
/// fails, `run_complete` falls back to the built-ins only, exit 0, nothing
/// on stderr to clobber the prompt.
#[cfg(unix)]
#[test]
fn symlinked_module_offers_only_built_ins_silently() {
    let tmp = fixture();
    let tools = tmp.path().join("tools");
    std::fs::create_dir(tmp.path().join("shared")).unwrap();
    std::fs::write(tmp.path().join("shared/linked.py"), "").unwrap();
    std::os::unix::fs::symlink("../shared/linked.py", tools.join("linked.py")).unwrap();

    let output = common::toolr()
        .current_dir(tmp.path())
        .arg("__complete")
        .arg(tmp.path())
        .arg("")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let values: Vec<&str> = stdout
        .lines()
        .map(|l| l.split('\t').next().unwrap_or(l))
        .collect();
    assert!(values.contains(&"self"), "{stdout}");
    assert!(!values.contains(&"ci"), "{stdout}");
}

#[test]
fn double_dash_and_dash_choices_through_the_binary() {
    let tmp = TempDir::new().unwrap();
    let tools = common::write_project(tmp.path());
    std::fs::write(
        tools.join("ci.py"),
        r#""""CI utilities."""
from typing import Literal

group = command_group("ci", "CI utilities", docstring=__doc__)

@group.command
def pick(ctx, opt: Literal["--dash", "-x", "plain"] = "plain"):
    """Pick one."""
    pass

@group.command
def deploy(ctx, env: Literal["staging", "production"]):
    """Deploy something."""
    pass
"#,
    )
    .unwrap();
    common::toolr()
        .current_dir(tmp.path())
        .arg("__build-static-manifest")
        .assert()
        .success();

    assert_eq!(
        complete(&tmp, &["ci", "deploy", "--", ""]),
        "production\nstaging",
        "choices of the positional after `--`"
    );
    assert_eq!(
        complete(&tmp, &["ci", "deploy", "--", "--"]),
        "",
        "no flag names after `--`"
    );
    assert_eq!(
        complete(&tmp, &["ci", "pick", "--opt", ""]),
        "plain",
        "dash-leading choices dropped in the separate form"
    );
    assert_eq!(
        complete(&tmp, &["ci", "pick", "--opt="]),
        "--dash\n-x\nplain",
        "dash-leading choices kept after `=`"
    );
}

#[test]
fn root_double_dash_is_a_terminator() {
    let tmp = fixture();
    // clap rejects `toolr -- ci`, so nothing completes after a committed root `--`.
    assert_eq!(complete(&tmp, &["--", ""]), "");
    assert_eq!(complete(&tmp, &["--", "ci", ""]), "");
}

fn sorted_lines(tmp: &TempDir, args: &[&str]) -> Vec<String> {
    let mut lines: Vec<String> = complete(tmp, args).lines().map(str::to_string).collect();
    lines.sort();
    lines
}

#[test]
fn completes_groups_after_a_root_flag() {
    let tmp = fixture();
    let lines = sorted_lines(&tmp, &["--quiet", ""]);
    for group in ["ci", "project", "self"] {
        assert!(lines.contains(&group.to_string()), "{lines:?}");
    }
    assert!(lines.iter().all(|l| !l.starts_with("--")), "{lines:?}");
}

#[test]
fn completes_group_prefix_after_a_root_flag() {
    let tmp = fixture();
    assert_eq!(sorted_lines(&tmp, &["--quiet", "c"]), ["ci"]);
}

#[test]
fn completes_commands_after_a_root_flag() {
    let tmp = fixture();
    let lines = sorted_lines(&tmp, &["--quiet", "ci", ""]);
    assert!(lines.contains(&"hello".to_string()), "{lines:?}");
    assert!(lines.contains(&"deploy".to_string()), "{lines:?}");
}

#[test]
fn completes_further_root_flags() {
    let tmp = fixture();
    assert_eq!(
        sorted_lines(&tmp, &["--quiet", "--"]),
        [
            "--help",
            "--no-output-timeout-secs",
            "--no-timestamps",
            "--timeout-secs",
            "--timestamps"
        ]
    );
}

#[test]
fn skips_conflicting_and_given_root_flags() {
    let tmp = fixture();
    assert_eq!(
        sorted_lines(&tmp, &["--quiet", "--timestamps", "--"]),
        ["--help", "--no-output-timeout-secs", "--timeout-secs"]
    );
}

#[test]
fn skips_the_conflict_of_a_given_alias() {
    let tmp = fixture();
    assert_eq!(
        sorted_lines(&tmp, &["--ts", "--no"]),
        ["--no-output-timeout-secs"]
    );
}

#[test]
fn counts_short_and_alias_flags_as_given() {
    let tmp = fixture();
    let lines = sorted_lines(&tmp, &["-q", "--"]);
    assert!(!lines.contains(&"--quiet".to_string()), "{lines:?}");
    assert!(!lines.contains(&"--debug".to_string()), "{lines:?}");
    assert!(lines.contains(&"--timestamps".to_string()), "{lines:?}");
    assert_eq!(
        sorted_lines(&tmp, &["--timeout", "5", "--t"]),
        ["--timestamps"]
    );
}

#[test]
fn completes_after_a_flag_value() {
    let tmp = fixture();
    let lines = sorted_lines(&tmp, &["--timeout-secs", "5", ""]);
    assert!(lines.contains(&"ci".to_string()), "{lines:?}");
    let lines = sorted_lines(&tmp, &["--timeout-secs=5", "--"]);
    assert!(!lines.contains(&"--timeout-secs".to_string()), "{lines:?}");
    assert!(lines.contains(&"--quiet".to_string()), "{lines:?}");
}

#[test]
fn completes_nothing_where_clap_rejects_root_flags() {
    let tmp = fixture();
    for line in [
        &["--quiet", "--debug", "--"][..],
        &["--quiet", "--quiet", "--"],
        &["-qd", ""],
        &["--timeout-secs", "--"],
        &["--timeout-secs", ""],
        &["--timeout-secs", "abc", ""],
    ] {
        assert_eq!(complete_raw(&tmp, line), "", "{line:?}");
    }
}

#[test]
fn root_flags_stay_root_only() {
    let tmp = fixture();
    assert_eq!(sorted_lines(&tmp, &["--quiet", "ci", "--"]), ["--help"]);
}

#[test]
fn help_given_is_not_offered_again() {
    let tmp = fixture();
    let lines = sorted_lines(&tmp, &["--help", "--"]);
    assert!(!lines.contains(&"--help".to_string()), "{lines:?}");
    assert!(lines.contains(&"--quiet".to_string()), "{lines:?}");
}

#[test]
fn terminator_blocks_root_flags() {
    let tmp = fixture();
    assert_eq!(complete_raw(&tmp, &["--", "--"]), "");
    assert_eq!(complete_raw(&tmp, &["--quiet", "--", "--"]), "");
}

#[test]
fn completes_after_an_attached_negative_value() {
    let tmp = fixture();
    for flag in ["--timeout-secs=-1", "--nots=-1"] {
        let lines = sorted_lines(&tmp, &[flag, ""]);
        assert!(lines.contains(&"ci".to_string()), "{flag}: {lines:?}");
    }
    let lines = sorted_lines(&tmp, &["--timeout-secs=-1", "--t"]);
    assert_eq!(lines, ["--timestamps"]);
}

// Regression guards: these lines printed nothing before the version/value fixes too (by accident, not
// design). `scan_version_ends_the_line` in `builtin_completions` is what proves the fix.
#[test]
fn stays_empty_after_version_or_bad_values() {
    let tmp = fixture();
    for line in [
        &["-V", ""][..],
        &["--version", ""],
        &["-V", "--"],
        &["--timeout-secs", "-1", ""],
    ] {
        assert_eq!(complete_raw(&tmp, line), "", "{line:?}");
    }
}
