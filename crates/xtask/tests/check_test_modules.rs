//! End-to-end checks for `cargo xtask check-test-modules`, as the pre-commit hook runs it.

use std::fs;

use assert_cmd::Command;

#[test]
fn passes_out_of_line_modules_and_names_an_inline_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let clean = dir.path().join("clean.rs");
    let inline = dir.path().join("inline.rs");
    fs::write(&clean, "#[cfg(test)]\nmod tests;\n").expect("write");
    fs::write(&inline, "#[cfg(all(unix, test))]\nmod unit {}\n").expect("write");

    Command::cargo_bin("xtask")
        .expect("xtask binary")
        .arg("check-test-modules")
        .arg(&clean)
        .assert()
        .success();

    let out = Command::cargo_bin("xtask")
        .expect("xtask binary")
        .arg("check-test-modules")
        .arg(&clean)
        .arg(&inline)
        .assert()
        .failure()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("inline.rs: inline test module `unit`"),
        "{stderr}"
    );
    assert!(!stderr.contains("clean.rs"), "{stderr}");
}

#[test]
fn without_files_checks_every_crate_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("crates/demo/src");
    fs::create_dir_all(src.join("nested")).expect("mkdir");
    fs::write(src.join("lib.rs"), "#[cfg(test)]\nmod tests;\n").expect("write");
    fs::write(src.join("nested/inline.rs"), "#[cfg(test)]\nmod unit {}\n").expect("write");

    let out = Command::cargo_bin("xtask")
        .expect("xtask binary")
        .arg("check-test-modules")
        .current_dir(dir.path())
        .assert()
        .failure()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("inline.rs: inline test module `unit`"),
        "{stderr}"
    );
    assert!(!stderr.contains("lib.rs"), "{stderr}");
}
