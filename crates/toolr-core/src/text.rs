//! Helpers for text that originates in a repository and ends up on a terminal.
//!
//! Each sink has its own policy:
//! - Names: rejected at build time (`parser/names.rs`), and dropped at load
//!   time by `Manifest::drop_unsafe_names`. That runs in `main.rs` and
//!   `complete/freshness.rs`, which must stay paired so clap and TAB agree.
//! - Completion values: dropped by the engine's `retain`.
//! - Choices and defaults: filtered by `printable_choices` and dropped in `cli.rs`.
//! - Clap help: [`strip_control_chars_per_line`].
//! - Warnings and descriptions: [`strip_control_chars`].
//! - Error text: [`escape_control_chars`].
//!
//! Names, choices, defaults and completion values also refuse invisible
//! format characters ([`has_unsafe_name_chars`]): a zero-width or bidi mark
//! makes one name look like another. Prose keeps them, since RTL text needs
//! LRM/RLM and emoji and Indic scripts need ZWJ.

fn is_control(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{1f}' | '\u{7f}' | '\u{80}'..='\u{9f}')
}

/// Zero-width and bidi format characters: ALM, ZWSP/ZWNJ/ZWJ, LRM/RLM, the
/// bidi embeddings, overrides and isolates, word joiner, invisible operators
/// and BOM.
fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{061c}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}'
    )
}

/// True for any C0 control, DEL or C1 control character.
pub fn has_control_chars(s: &str) -> bool {
    s.chars().any(is_control)
}

/// True for any zero-width or bidi format character.
pub fn has_invisible_format_chars(s: &str) -> bool {
    s.chars().any(is_invisible_format)
}

/// True when a name or value carries a control or invisible format character.
pub fn has_unsafe_name_chars(s: &str) -> bool {
    s.chars().any(|c| is_control(c) || is_invisible_format(c))
}

/// Replace each control character with a space, then collapse runs of spaces
/// to one. The space keeps the word boundary a stripped `\n` or `\t` marked.
pub fn strip_control_chars(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        let c = if is_control(c) { ' ' } else { c };
        if c == ' ' && prev_space {
            continue;
        }
        prev_space = c == ' ';
        out.push(c);
    }
    out
}

/// Strip per line, so clean lines keep their indentation and inner spacing
/// verbatim. Clap passes ANSI through in its own error output.
pub fn strip_control_chars_per_line(s: &str) -> String {
    s.split('\n')
        .map(|l| {
            if has_control_chars(l) {
                strip_control_chars(l)
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render control and invisible format characters as `\u{..}` and leave
/// everything else alone.
pub fn escape_control_chars(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if is_control(c) || is_invisible_format(c) {
            out.extend(c.escape_unicode());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
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
}
