use super::*;

#[test]
fn reads_the_first_value_for_a_key() {
    let tmp = tempfile::TempDir::new().unwrap();
    fs::write(
        tmp.path().join("pyvenv.cfg"),
        "home = /usr/bin\nnot a pair\nversion_info = 3.13.2\nhome = /other\n",
    )
    .unwrap();
    assert_eq!(
        pyvenv_cfg_value(tmp.path(), "home").as_deref(),
        Some("/usr/bin")
    );
    assert_eq!(
        pyvenv_cfg_value(tmp.path(), "version_info").as_deref(),
        Some("3.13.2")
    );
    assert_eq!(pyvenv_cfg_value(tmp.path(), "version"), None);
    assert_eq!(pyvenv_cfg_value(&tmp.path().join("nope"), "home"), None);
}
