//! Helpers for text that originates in a repository and ends up on a terminal.
//!
//! Each sink has its own policy:
//! - Names: rejected at build time (`parser/names.rs`), and dropped at load
//!   time by `Manifest::drop_control_char_names`. That runs in `main.rs` and
//!   `complete/freshness.rs`, which must stay paired so clap and TAB agree.
//! - Completion values: dropped by the engine's `retain`.
//! - Choices and defaults: filtered by `printable_choices` and dropped in `cli.rs`.
//! - Clap help: [`strip_control_chars_per_line`].
//! - Warnings and descriptions: [`strip_control_chars`].
//! - Error text: [`escape_control_chars`].

fn is_control(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{1f}' | '\u{7f}' | '\u{80}'..='\u{9f}')
}

/// True for any C0 control, DEL or C1 control character.
pub fn has_control_chars(s: &str) -> bool {
    s.chars().any(is_control)
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

/// Render control characters as `\u{..}` and leave everything else alone.
pub fn escape_control_chars(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if is_control(c) {
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
}
