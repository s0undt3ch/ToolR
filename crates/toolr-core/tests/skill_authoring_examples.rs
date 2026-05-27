//! Manifest snapshot for the toolr-command-authoring skill's examples
//! tree. The committed `toolr-manifest.json` is the source of truth
//! for what shape an agent reading the skill should expect to see.
//! Any change to toolr's command-authoring surface that mutates
//! manifest output requires regenerating the snapshot.
//!
//! Regenerate locally:
//!
//! ```sh
//! cargo test -p toolr-core --test skill_authoring_examples -- \
//!     --ignored regenerate
//! ```

use std::fs;
use std::path::PathBuf;

use toolr_core::manifest::Manifest;
use toolr_core::parser::build_static_manifest;

#[test]
fn examples_manifest_matches_committed_snapshot() {
    let examples = examples_dir();
    let manifest = build_static_manifest(&examples.join("tools"))
        .expect("building the example tools/ manifest");
    let actual = serialise(&manifest);
    let snapshot_path = examples.join("toolr-manifest.json");
    let expected = fs::read_to_string(&snapshot_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", snapshot_path.display()));
    assert_eq!(
        actual.trim_end(),
        expected.trim_end(),
        "skill example manifest drifted from the committed snapshot at {}. \
         Regenerate with `cargo test -p toolr-core --test \
         skill_authoring_examples -- --ignored regenerate` and commit the \
         result.",
        snapshot_path.display(),
    );
}

#[test]
#[ignore = "explicit regeneration only"]
fn regenerate() {
    let examples = examples_dir();
    let manifest = build_static_manifest(&examples.join("tools"))
        .expect("building the example tools/ manifest");
    let snapshot_path = examples.join("toolr-manifest.json");
    let body = format!("{}\n", serialise(&manifest));
    fs::write(&snapshot_path, body)
        .unwrap_or_else(|e| panic!("write {}: {e}", snapshot_path.display()));
}

fn serialise(manifest: &Manifest) -> String {
    serde_json::to_string_pretty(manifest).expect("serialising the example manifest")
}

fn examples_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR points at `crates/toolr-core/`; walk up to the
    // workspace root and into `skills/toolr-command-authoring/examples`.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .ancestors()
        .nth(2)
        .map(|root| {
            root.join("skills/toolr-command-authoring/examples")
        })
        .expect("workspace root two levels above CARGO_MANIFEST_DIR")
}
