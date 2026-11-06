//! A group bound through an aliased `command_group`, or with an annotated
//! assignment, reaches `--help` with its commands.

mod common;

use common::Project;

#[test]
fn aliased_and_annotated_groups_list_their_commands() {
    let p = Project::new();
    p.write_tool(
        "aliased.py",
        r#"from toolr import command_group as cg
from toolr._decorators import CommandGroup

g = cg("aliased", "Aliased group", "Aliased group.")
h: CommandGroup = cg("annotated", "Annotated group", "Annotated group.")


@g.command
def alpha(ctx):
    """Run alpha."""


@h.command
def beta(ctx):
    """Run beta."""
"#,
    );
    let top = p.stdout(&["--help"]);
    assert!(top.contains("aliased"), "{top}");
    assert!(top.contains("annotated"), "{top}");
    let aliased = p.stdout(&["aliased", "--help"]);
    assert!(aliased.contains("alpha"), "{aliased}");
    let annotated = p.stdout(&["annotated", "--help"]);
    assert!(annotated.contains("beta"), "{annotated}");
}
