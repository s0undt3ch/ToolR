use std::fs;

use super::*;

fn found(source: &str) -> Vec<String> {
    inline_test_modules(source).expect("parse")
}

#[test]
fn flags_every_shape_of_inline_test_module() {
    for source in [
        "#[cfg(test)]\nmod tests {}",
        "#[cfg(test)] pub(crate) mod tests {}",
        "#[cfg(test)]\nmod tests\n{\n}",
        "#[cfg(test)]\n#[cfg(unix)]\nmod tests {}",
        "#[cfg(unix)]\n#[cfg(test)]\nmod tests {}",
        "#[cfg(all(test, unix))]\nmod tests {}",
        "#[cfg(all(unix, test))]\nmod tests {}",
        "#[cfg(any(test, feature = \"x\"))]\nmod tests {}",
        "#[cfg(all(unix, any(windows, test)))]\nmod tests {}",
        "#[cfg(test)]\n// why\n/* and */\n/// doc\n#[allow(dead_code)]\nmod tests {}",
    ] {
        assert_eq!(found(source), ["tests"], "{source}");
    }
}

#[test]
fn allows_out_of_line_modules_and_non_test_code() {
    for source in [
        "#[cfg(test)]\nmod tests;",
        "#[cfg(all(test, unix))]\nmod tests;",
        "#[cfg(test)]\nfn helper() {}",
        "#[cfg(any(windows, test))]\nfn helper() {}",
        "#[cfg(not(test))]\nmod real {}",
        "#[cfg(all(unix, not(test)))]\nmod real {}",
        "#[cfg(feature = \"test\")]\nmod real {}",
        "#[cfg(testing)]\nmod real {}",
        "#[cfg_attr(test, allow(dead_code))]\nmod real {}",
        "const S: &str = \"#[cfg(test)] mod tests {\";",
        "// #[cfg(test)] mod tests {}",
    ] {
        assert!(found(source).is_empty(), "{source}");
    }
}

#[test]
fn finds_test_modules_nested_in_other_inline_modules() {
    let source = "mod outer {\n    #[cfg(test)]\n    mod inner {}\n}\n#[cfg(test)]\nmod tests {}";
    assert_eq!(found(source), ["inner", "tests"]);
}

#[test]
fn does_not_look_inside_a_test_module() {
    assert_eq!(
        found("#[cfg(test)]\nmod tests {\n    #[cfg(test)]\n    mod deeper {}\n}"),
        ["tests"]
    );
}

#[test]
fn run_passes_clean_files_and_fails_on_an_inline_module() {
    let dir = tempfile::tempdir().expect("tempdir");
    let clean = dir.path().join("clean.rs");
    let inline = dir.path().join("inline.rs");
    fs::write(&clean, "#[cfg(test)]\nmod tests;\n").expect("write");
    fs::write(&inline, "#[cfg(test)]\nmod tests {}\n").expect("write");

    run(std::slice::from_ref(&clean)).expect("clean file passes");
    let err = run(&[clean, inline]).expect_err("inline module fails");
    assert!(err.to_string().contains("tests.rs"), "{err}");
}

#[test]
fn run_reports_unreadable_and_unparsable_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = dir.path().join("missing.rs");
    let broken = dir.path().join("broken.rs");
    fs::write(&broken, "fn {").expect("write");

    let err = run(&[missing]).expect_err("missing file fails");
    assert!(format!("{err:#}").starts_with("read "), "{err:#}");
    let err = run(&[broken]).expect_err("broken file fails");
    assert!(format!("{err:#}").starts_with("parse "), "{err:#}");
}
