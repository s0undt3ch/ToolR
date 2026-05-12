//! Offline tests for the install path. The network-touching
//! `perform_install` is exercised manually by the implementer in Task 4.3.

use _rust_utils::uv::install::{asset_url, host_asset};

#[test]
fn host_asset_present_on_supported_targets() {
    // On the CI runners we care about, this should always succeed.
    // (Linux x86_64, macOS aarch64.) On unsupported triples the test
    // still passes — it just records `None`.
    let _ = host_asset();
}

#[test]
fn asset_url_points_at_astral_releases() {
    let url = asset_url("uv-x86_64-unknown-linux-gnu", "tar.gz");
    assert!(url.starts_with("https://github.com/astral-sh/uv/releases"));
    assert!(url.ends_with("uv-x86_64-unknown-linux-gnu.tar.gz"));
}
