use super::*;

#[test]
fn strip_removes_esc_and_c1() {
    let out = strip_control_chars("a\x1b]0;x\x07b\u{9b}c");
    assert!(!has_control_chars(&out), "{out:?}");
}

#[test]
fn strip_collapses_spaces() {
    assert_eq!(strip_control_chars("a\n\n b"), "a b");
}

#[test]
fn per_line_strip_keeps_clean_lines_verbatim() {
    let out = strip_control_chars_per_line("    indented  two\nbad\x1b]0;x\x07 line");
    assert_eq!(out, "    indented  two\nbad ]0;x line");
}

#[test]
fn has_control_chars_flags_only_controls() {
    assert!(has_control_chars("\x7f"));
    assert!(!has_control_chars("dry run"));
    assert!(!has_control_chars("$(x)"));
}

#[test]
fn unsafe_name_chars_cover_controls_and_invisible_format() {
    for bad in [
        "de\u{200b}ploy",
        "a\u{061c}",
        "\u{202e}gnp.exe",
        "x\u{2064}",
        "\u{2069}",
        "\u{feff}bom",
        "\x1b",
    ] {
        assert!(has_unsafe_name_chars(bad), "{bad:?}");
    }
    assert!(!has_unsafe_name_chars("deploy"));
    assert!(!has_unsafe_name_chars("r\u{e9}sum\u{e9}"));
    assert!(!has_unsafe_name_chars("x\u{2065}"));
    assert!(has_invisible_format_chars("\u{200d}"));
    assert!(!has_invisible_format_chars("\x1b"));
}

#[test]
fn escape_renders_invisible_format_chars() {
    assert_eq!(escape_control_chars("de\u{200b}ploy"), "de\\u{200b}ploy");
    assert_eq!(escape_control_chars("a\u{202e}b"), "a\\u{202e}b");
}

#[test]
fn prose_strip_keeps_invisible_format_chars() {
    let rtl = "x\u{200f}y \u{1f468}\u{200d}\u{1f469}";
    assert_eq!(strip_control_chars(rtl), rtl);
    assert_eq!(strip_control_chars_per_line(rtl), rtl);
}
