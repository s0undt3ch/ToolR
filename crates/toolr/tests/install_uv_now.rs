//! The hidden `__install-uv-now` command, with uv already on `PATH`. The
//! download path isn't exercised: it needs the network.

#![cfg(unix)]

mod common;

use std::process::Command;

use common::VenvFixture;
use tempfile::TempDir;

#[test]
fn install_uv_now_reports_the_uv_already_on_path() {
    let fx = VenvFixture::new();
    let data = TempDir::new().unwrap();
    let out = Command::new(assert_cmd::cargo::cargo_bin("toolr"))
        .arg("__install-uv-now")
        .current_dir(&fx.root)
        .env("PATH", &fx.bin_dir)
        .env("XDG_DATA_HOME", data.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("toolr: uv 0.5.0 ready at"), "{stdout}");
    assert!(
        stdout.contains("Path"),
        "expected the PATH uv, got:\n{stdout}"
    );
}
