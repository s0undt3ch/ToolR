//! A command built from `tools/` and the same command shipped in a plugin fragment must merge
//! into identical manifest entries, bar the module prefix and the origin.

use std::fs;
use std::path::Path;

use tempfile::TempDir;
use toolr_core::manifest::{ArgumentKind, Command, Group, Manifest, Origin, SCHEMA_VERSION};
use toolr_core::parser::{SupportedType, build_static_manifest};
use toolr_core::third_party::discover_and_merge;
use toolr_core::{build_third_party_fragment, serialise_fragment};

const ENUMS_PY: &str = r#""""Enums."""
import enum


class Color(enum.Enum):
    RED = "red"
    BLUE = "blue"
"#;

const COMMANDS_PY: &str = r#""""Docker commands."""
from typing import Annotated

from toolr import arg, arg_section, command_group
from toolr.types import Email, FilePath

from .enums import Color

OUTPUT = arg_section("Output", description="Where results go.")

docker = command_group("docker", "Docker", "Docker tools.")
image = docker.command_group("image", "Image", "Image tools.")
spare = command_group("spare", "Spare", "No commands here.")


@image.command
def build(
    ctx,
    pair: tuple[str, int] | None = None,
    colour: Color = Color.RED,
    to: Email | None = None,
    out: Annotated[
        FilePath | None,
        arg(aliases=["-o"], metavar="FILE", env="PAR_OUT", help_section=OUTPUT),
    ] = None,
    count: int = 1,
) -> None:
    """Build an image.

    Builds the image from the local context.

    Args:
        pair: A name and a number.
        colour: The colour to paint it.
        to: Who to notify.
        out: Where to write the result.
        count: How many times to build.
    """
"#;

fn write_tree(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("__init__.py"), "").unwrap();
    fs::write(dir.join("enums.py"), ENUMS_PY).unwrap();
    fs::write(dir.join("commands.py"), COMMANDS_PY).unwrap();
}

fn empty_base() -> Manifest {
    Manifest {
        schema_version: SCHEMA_VERSION,
        static_hash: String::new(),
        third_party_hash: String::new(),
        toolr_version: String::new(),
        groups: vec![],
        commands: vec![],
    }
}

fn normalised(manifest: &Manifest, origin: Origin) -> String {
    let mut groups: Vec<&Group> = manifest
        .groups
        .iter()
        .filter(|g| g.origin == origin)
        .collect();
    groups.sort_by_key(|g| g.full_path());
    let mut commands: Vec<&Command> = manifest
        .commands
        .iter()
        .filter(|c| c.origin == origin)
        .collect();
    commands.sort_by(|a, b| (&a.group, &a.name).cmp(&(&b.group, &b.name)));
    let value = serde_json::json!({ "groups": groups, "commands": commands });
    serde_json::to_string_pretty(&value).unwrap()
}

#[test]
fn a_command_behaves_the_same_built_locally_and_as_a_plugin() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    let pkg = tmp.path().join("parpkg");
    write_tree(&tools);
    write_tree(&pkg);

    let local = build_static_manifest(&tools).unwrap();

    let fragment = build_third_party_fragment(&pkg, "parpkg").unwrap();
    let paths: Vec<(String, Option<String>)> = fragment
        .groups
        .iter()
        .map(|g| (g.full_path(), g.parent.clone()))
        .collect();
    assert_eq!(
        paths,
        [
            ("docker".to_string(), None),
            ("docker.image".to_string(), Some("docker".to_string())),
            ("spare".to_string(), None),
        ]
    );
    let pair = fragment.commands[0]
        .arguments
        .iter()
        .find(|a| a.name == "pair")
        .unwrap();
    assert_eq!(pair.kind, ArgumentKind::Repeated);
    assert_eq!(
        pair.resolved_type,
        Some(SupportedType::Optional(Box::new(SupportedType::Tuple(
            vec![SupportedType::Str, SupportedType::Int,]
        ))))
    );

    let venv = tmp.path().join("venv");
    let site = venv
        .join("lib")
        .join("python3.13")
        .join("site-packages")
        .join("parpkg");
    fs::create_dir_all(&site).unwrap();
    fs::write(
        site.join("toolr-manifest.json"),
        serialise_fragment(&fragment).unwrap(),
    )
    .unwrap();
    let plugin = discover_and_merge(&venv, empty_base()).unwrap();

    let local_side = normalised(&local, Origin::Static)
        .replace("\"tools.", "\"parpkg.")
        .replace("\"tools\"", "\"parpkg\"");
    let plugin_side =
        normalised(&plugin, Origin::ThirdParty).replace("\"third_party\"", "\"static\"");
    assert_eq!(local_side, plugin_side);
}
