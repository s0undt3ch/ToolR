//! CI gates for Rust settings repeated outside `Cargo.toml`, which nothing else keeps in step.

use std::fs;
use std::path::{Path, PathBuf};

/// The workspace `rust-version` must be the toolchain mise pins, the only Rust that builds and
/// tests toolr. Cargo then refuses any dependency that needs a newer one.
#[test]
fn workspace_rust_version_matches_mise_toolchain() {
    let root = workspace_root();
    let mise = read_toml(&root.join("mise.toml"));
    let pinned = mise["tools"]["rust"]["version"]
        .as_str()
        .expect("mise.toml pins tools.rust.version");
    let declared = workspace_package(&root)["rust-version"]
        .as_str()
        .expect("Cargo.toml declares workspace.package.rust-version")
        .to_owned();

    assert_eq!(
        declared, pinned,
        "workspace rust-version is {declared:?} but mise.toml pins Rust {pinned:?}. Renovate's \
         \"Rust toolchain\" group bumps both; by hand, set `rust-version` in the root Cargo.toml \
         to the pinned version and add a release note with `toolr release-notes add`.",
    );
}

/// The pre-commit hook runs rustfmt per file, which reads `.rustfmt.toml` and not `Cargo.toml`.
#[test]
fn rustfmt_edition_matches_workspace_edition() {
    let root = workspace_root();
    let edition = workspace_package(&root)["edition"]
        .as_str()
        .expect("Cargo.toml declares workspace.package.edition")
        .to_owned();
    let rustfmt = read_toml(&root.join(".rustfmt.toml"));

    assert_eq!(
        rustfmt["edition"].as_str(),
        Some(edition.as_str()),
        "set `edition` in .rustfmt.toml to the workspace edition {edition:?}",
    );
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .expect("workspace root two levels above CARGO_MANIFEST_DIR")
}

fn workspace_package(root: &Path) -> toml::Table {
    read_toml(&root.join("Cargo.toml"))["workspace"]["package"]
        .as_table()
        .expect("Cargo.toml has [workspace.package]")
        .clone()
}

fn read_toml(path: &Path) -> toml::Table {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .parse()
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}
