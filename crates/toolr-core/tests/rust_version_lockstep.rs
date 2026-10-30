//! CI gate: the workspace `rust-version` must be the toolchain mise pins, the only Rust that
//! builds and tests toolr. Cargo then refuses any dependency that needs a newer one.

use std::fs;
use std::path::Path;

#[test]
fn workspace_rust_version_matches_mise_toolchain() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("toolr-core lives under crates/");

    let mise = read_toml(&root.join("mise.toml"));
    let pinned = mise["tools"]["rust"]["version"]
        .as_str()
        .expect("mise.toml pins tools.rust.version");

    let cargo = read_toml(&root.join("Cargo.toml"));
    let declared = cargo["workspace"]["package"]["rust-version"]
        .as_str()
        .expect("Cargo.toml declares workspace.package.rust-version");

    assert_eq!(
        declared, pinned,
        "workspace rust-version is {declared:?} but mise.toml pins Rust {pinned:?}. Renovate's \
         \"Rust toolchain\" group bumps both; by hand, set `rust-version` in the root Cargo.toml \
         to the pinned version and note the new minimum Rust in UNRELEASED.md.",
    );
}

fn read_toml(path: &Path) -> toml::Table {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        .parse()
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}
