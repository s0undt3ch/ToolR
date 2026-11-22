use super::*;

#[test]
fn crlf_region_extracts_byte_identically_to_lf() {
    let src = "a\n// region: T\nstruct S {\n    x: u8,\n}\n\n// endregion: T\nb\n";
    let lf = extract_region(src, "f.rs", "T").unwrap();
    assert_eq!(lf, "struct S {\n    x: u8,\n}\n");
    assert_eq!(
        extract_region(&src.replace('\n', "\r\n"), "f.rs", "T").unwrap(),
        lf
    );
}

#[test]
fn region_from_a_file_without_an_embedded_copy_is_an_error() {
    let err = embedded("nope.rs").unwrap_err().to_string();
    assert!(err.contains("no embedded copy of `nope.rs`"), "{err}");
}
