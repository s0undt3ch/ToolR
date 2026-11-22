use super::*;
use std::io::Write;
use tempfile::NamedTempFile;

fn write_tmp(source: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(source.as_bytes()).unwrap();
    f
}

#[test]
fn syntax_error_names_the_file_message_line_and_column() {
    let f = write_tmp("x = 1\ndef not closed(\n");
    let err = parse_python_file(f.path()).unwrap_err().to_string();
    let head = format!("parsing {}: ", f.path().display());
    let rest = err.strip_prefix(&head).expect(&err);
    assert!(rest.ends_with("(line 2, column 5)"), "{rest}");
    assert!(!rest.contains("byte range"), "{rest}");
}

#[test]
fn syntax_error_text_has_no_raw_control_characters() {
    let f = write_tmp("\x1b]0;pwn\x07 = 1\n");
    let err = parse_python_file(f.path()).unwrap_err().to_string();
    assert!(!err.chars().any(char::is_control), "{err:?}");
    assert!(err.contains("\\u{1b}"), "{err}");
}

#[test]
fn syntax_error_text_escapes_bidi_marks() {
    let f = write_tmp("a\u{202e}b = 1\n");
    let err = parse_python_file(f.path()).unwrap_err().to_string();
    assert!(!err.contains('\u{202e}'), "{err:?}");
    assert!(err.contains("\\u{202e}"), "{err}");
}

#[test]
fn line_and_column_counts_characters_and_clamps() {
    assert_eq!(line_and_column("", 0), (1, 1));
    assert_eq!(line_and_column("ab\ncd", 4), (2, 2));
    assert_eq!(line_and_column("é=\n", 2), (1, 2));
    // Inside the two-byte `é`, and past the end.
    assert_eq!(line_and_column("é=", 1), (1, 1));
    assert_eq!(line_and_column("a\n", 99), (2, 1));
}

#[test]
fn parses_a_simple_module() {
    let f = write_tmp("x = 1\n");
    let module = parse_python_file(f.path()).expect("should parse");
    assert!(!module.body.is_empty());
}

#[test]
fn returns_error_on_syntax_error() {
    let f = write_tmp("def broken(\n");
    let err = parse_python_file(f.path()).expect_err("should fail");
    assert!(err.to_string().contains("parsing"));
}
