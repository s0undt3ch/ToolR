//! What toolr reads from its own argv before clap runs: only the leading run of root flags is
//! toolr's, so a command's own `--version` or a root flag before a built-in is not misread.

mod common;

use common::Project;

const REL_GROUP: &str = "\"\"\"Release.\"\"\"\nfrom toolr import Context, command_group\n\
     group = command_group(\"rel\", \"Release\", \"Release tools.\")\n\
     @group.command\ndef bump(ctx: Context, version: str = \"\") -> None:\n    \
     \"\"\"Bump the version.\n\n    Args:\n        version: The new version.\n    \"\"\"\n";

#[test]
fn a_command_with_its_own_version_option_builds_the_manifest_on_a_fresh_clone() {
    let p = Project::new();
    p.write_tool("rel.py", REL_GROUP);
    assert!(!p.tools().join(".toolr-manifest.json").exists());

    let out = p.stdout(&["rel", "bump", "--version", "1.2", "--help"]);
    assert!(out.contains("The new version."), "{out}");
}

#[test]
fn root_flags_before_a_built_in_keep_its_help() {
    let p = Project::new();
    for (args, about) in [
        (&["-q", "self", "--help"][..], "Operations on toolr itself"),
        (
            &["--timeout-secs", "5", "project", "venv", "-h"][..],
            "Inspect, sync, and operate on the tools venv",
        ),
    ] {
        let out = p.stdout(args);
        assert!(out.starts_with(about), "{args:?}:\n{out}");
    }
}

#[test]
fn version_does_not_read_the_manifest() {
    let p = Project::new();
    std::fs::write(p.tools().join(".toolr-manifest.json"), "{bad").unwrap();

    for args in [&["--version"][..], &["-V"]] {
        let out = p.toolr(args).output().unwrap();
        assert!(out.status.success(), "{args:?}");
        assert!(
            out.stderr.is_empty(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(String::from_utf8(out.stdout).unwrap().starts_with("toolr "));
    }
}

#[test]
fn a_user_group_named_like_a_hidden_built_in_is_ignored_with_a_warning() {
    for name in ["__install-uv-now", "__complete", "self"] {
        let p = Project::new();
        p.write_tool(
            "u.py",
            &format!(
                "\"\"\"U.\"\"\"\nfrom toolr import Context, command_group\n\
                 group = command_group(\"{name}\", \"U\", \"U tools.\")\n\
                 @group.command\ndef go(ctx: Context) -> None:\n    \"\"\"Go.\"\"\"\n"
            ),
        );
        let err = p.stderr(&["--help"]);
        assert!(
            err.contains(&format!("ignoring user-defined group `{name}`")),
            "{name}: {err}"
        );
    }
}

#[test]
fn completion_does_not_offer_user_groups_reserved_by_built_ins() {
    let p = Project::new();
    for (file, name) in [("a.py", "__install-uv-now"), ("b.py", "self")] {
        p.write_tool(
            file,
            &format!(
                "\"\"\"U.\"\"\"\nfrom toolr import Context, command_group\n\
                 group = command_group(\"{name}\", \"U\", \"U tools.\")\n\
                 @group.command\ndef go(ctx: Context) -> None:\n    \"\"\"Go.\"\"\"\n"
            ),
        );
    }
    let cwd = p.tools().parent().unwrap().to_str().unwrap().to_owned();
    // Tab never builds the manifest; a normal run does.
    p.stderr(&["--help"]);
    let complete = |words: &[&str]| {
        let mut args = vec!["__complete", cwd.as_str()];
        args.extend(words);
        let out = p.toolr(&args).output().unwrap();
        assert!(
            out.stderr.is_empty(),
            "{words:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    };

    assert!(!complete(&["__"]).contains("__install-uv-now"));
    let after_self = complete(&["self", ""]);
    assert!(
        !after_self.lines().any(|l| l.starts_with("go\t")),
        "{after_self}"
    );
    assert!(after_self.contains("cache"), "{after_self}");
    assert!(complete(&["__install-uv-now", ""]).trim().is_empty());
}
