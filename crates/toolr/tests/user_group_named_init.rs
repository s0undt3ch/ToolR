//! A user command group may be named `init`: only `self`, `project` and
//! `__complete` are toolr's own top-level names, so a fresh clone (no manifest
//! yet) must still build one and show the group's help.

mod common;

use common::Project;

const INIT_GROUP: &str = "\"\"\"Init.\"\"\"\nfrom toolr import command_group\n\
     group = command_group(\"init\", \"Initialisation\", \"Initialisation.\")\n\
     @group.command\ndef hello(ctx):\n    \"\"\"Say hello.\"\"\"\n";

#[test]
fn user_group_named_init_is_reachable_on_a_fresh_clone() {
    let p = Project::new();
    p.write_tool("init.py", INIT_GROUP);
    assert!(!p.tools().join(".toolr-manifest.json").exists());

    let out = p.stdout(&["init", "--help"]);
    assert!(
        out.contains("hello"),
        "group help missing the command:\n{out}"
    );
}
