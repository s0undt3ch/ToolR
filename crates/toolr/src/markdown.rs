//! Markdown → ANSI rendering for `--help` output.
//!
//! Google-style docstrings are markdown-flavoured prose. By passing them
//! through `termimad` before handing them to clap we get inline-code
//! highlighting, bullet lists, headings, emphasis, and tables —
//! something close to a rich-rendered help page — bullet lists,
//! code spans, tables, headings.
//!
//! `termimad` only emits ANSI when stdout (or whatever `is_terminal`
//! reports) is a TTY; when the output is being captured or piped we
//! return the raw text so downstream tooling sees no escape sequences.

use std::io::IsTerminal;
use std::sync::OnceLock;

use termimad::MadSkin;

fn skin() -> &'static MadSkin {
    // `default_dark` works on both light- and dark-themed terminals
    // since it sticks to terminal palette colors rather than RGB.
    static SKIN: OnceLock<MadSkin> = OnceLock::new();
    SKIN.get_or_init(MadSkin::default)
}

/// Render `text` (Google-docstring markdown) to ANSI-styled text for
/// clap to display. On a non-TTY (CI, piped output, redirection) returns
/// the raw text so captured `.txt` snippets stay clean.
pub fn render(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    if !std::io::stdout().is_terminal() {
        // RST-style double backticks still get normalised so the
        // captured output reads cleanly even in plain-text form.
        return normalize_rst_backticks(text);
    }
    let normalised = normalize_rst_backticks(text);
    skin().text(&normalised, None).to_string()
}

/// Sphinx / RST docstrings use double backticks for inline code
/// (``kubectl``); commonplace markdown uses single backticks (`kubectl`).
/// We pre-normalise so users coming from either world get rendered
/// inline code without rewriting their docstrings.
///
/// Heuristic: replace a *pair* of backticks (exactly two) with a
/// single backtick. Triple-or-more sequences are left alone so fenced
/// code blocks (``` ``` ```) and unusual escapes survive.
fn normalize_rst_backticks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '`' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        // Count consecutive backticks.
        let start = i;
        while i < chars.len() && chars[i] == '`' {
            i += 1;
        }
        let run = i - start;
        if run == 2 {
            out.push('`');
        } else {
            for _ in 0..run {
                out.push('`');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rst_double_backticks_collapse_to_single() {
        assert_eq!(normalize_rst_backticks("``kubectl``"), "`kubectl`");
        assert_eq!(
            normalize_rst_backticks("Reads ``kubectl`` output"),
            "Reads `kubectl` output"
        );
    }

    #[test]
    fn single_backticks_are_preserved() {
        assert_eq!(normalize_rst_backticks("`already markdown`"), "`already markdown`");
    }

    #[test]
    fn triple_backticks_stay_for_fenced_code_blocks() {
        // Leave fenced code blocks alone — those are still markdown.
        assert_eq!(normalize_rst_backticks("```rust"), "```rust");
        assert_eq!(normalize_rst_backticks("```"), "```");
    }

    #[test]
    fn mixed_runs_are_each_evaluated_individually() {
        // `single` + ``double`` + ```triple``` — only the middle pair collapses.
        assert_eq!(
            normalize_rst_backticks("`a` ``b`` ```c```"),
            "`a` `b` ```c```"
        );
    }

    #[test]
    fn empty_input_is_empty_output() {
        assert_eq!(render(""), "");
    }

    /// Regression: an earlier byte-by-byte implementation mangled
    /// multi-byte UTF-8 codepoints (em-dash `—` came out as `â `).
    /// Iterate by `chars()` so non-ASCII content passes through whole.
    #[test]
    fn multibyte_utf8_is_preserved() {
        assert_eq!(
            normalize_rst_backticks("hello — ``world`` — café"),
            "hello — `world` — café"
        );
    }
}
