//! Helpers for text that originates in a repository and ends up on a terminal.

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
    fn has_control_chars_flags_only_controls() {
        assert!(has_control_chars("\x7f"));
        assert!(!has_control_chars("dry run"));
        assert!(!has_control_chars("$(x)"));
    }
}
