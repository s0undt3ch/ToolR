//! `third_party_hash` must not move for venvs whose fragments toolr already found: a changed
//! hash rebuilds every user's manifest. The fixtures are checked in under `tests/fixtures/` and
//! named relative to the crate root (cargo's cwd for tests), so the hashed paths are fixed.
//! Unix only: Windows joins the found paths with `\`, so the hashed bytes differ there.
#![cfg(all(unix, feature = "full"))]

use std::path::Path;

use toolr_core::manifest_build::compute_third_party_hash;

const FIXTURES: &str = "tests/fixtures/third-party-hash";

/// The uv layout. Both values are the ones the glob-based lookup computed before.
#[test]
fn uv_unix_layout_hash_is_pinned() {
    let hash = compute_third_party_hash(&Path::new(FIXTURES).join("uv-unix")).unwrap();
    assert_eq!(
        hash,
        "2efdc881417a80ddf1c83ebc7fe136dc5de5c334d1fa5a318211c9cc87c2f915"
    );
}

/// The Windows `Lib/site-packages` layout, which that glob also matched on every platform.
#[test]
fn windows_layout_hash_is_pinned() {
    let hash = compute_third_party_hash(&Path::new(FIXTURES).join("windows")).unwrap();
    assert_eq!(
        hash,
        "7401bf7cde6ee043fddc5b7cf349b316294a9f36f7e45d6f2379278a08605a65"
    );
}
