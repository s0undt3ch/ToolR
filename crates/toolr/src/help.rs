//! Render styled `--help` output by feeding docstring markdown through
//! `termimad` end-to-end. clap's built-in help renderer is bypassed
//! (see `cli.rs` and `dispatch.rs`); this module is the renderer.

use termimad::MadSkin;

#[allow(dead_code)] // wired up in Task 6
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HelpMode {
    /// `-h` — title + summary + usage + condensed options + subcommands.
    Short,
    /// `--help` — full markdown body + verbose options + subcommands + bugs.
    Long,
}

/// Resolve the rendering width.
///
/// Order: `$COLUMNS` if set and parseable; otherwise
/// `termimad::terminal_size().0 as usize`. Honoring `$COLUMNS` keeps
/// `regen-doc-snippets.py` (which pins `COLUMNS=100`) producing stable
/// captures even when stdout is not a TTY and crossterm's ioctl
/// fallback would return termimad's 50-col default.
#[allow(dead_code)] // wired up in Task 5
fn resolve_width() -> usize {
    resolve_width_from(std::env::var("COLUMNS").ok().as_deref(), || {
        termimad::terminal_size().0 as usize
    })
}

fn resolve_width_from(env_columns: Option<&str>, fallback: impl FnOnce() -> usize) -> usize {
    env_columns
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or_else(fallback)
}

/// Pick a `MadSkin` for the given output context. Caller decides
/// `is_tty` (from `std::io::stdout().is_terminal()`) and `no_color`
/// (from `NO_COLOR` env presence). Non-TTY or `NO_COLOR` set ⇒
/// plain skin (no ANSI; markdown structure still renders).
#[allow(dead_code)] // wired up in Task 5
fn skin_for(is_tty: bool, no_color: bool) -> MadSkin {
    if !is_tty || no_color {
        MadSkin::no_style()
    } else {
        MadSkin::default_dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tty_no_color_unset_returns_styled_skin() {
        let s = skin_for(true, false);
        let plain = MadSkin::no_style();
        assert!(s.bold != plain.bold, "default_dark should differ from no_style on bold styling");
    }

    #[test]
    fn non_tty_returns_plain_skin() {
        let s = skin_for(false, false);
        let plain = MadSkin::no_style();
        assert_eq!(s.bold, plain.bold);
    }

    #[test]
    fn no_color_set_returns_plain_skin_even_on_tty() {
        let s = skin_for(true, true);
        let plain = MadSkin::no_style();
        assert_eq!(s.bold, plain.bold);
    }

    #[test]
    fn columns_env_parses_to_width() {
        assert_eq!(resolve_width_from(Some("100"), || 50), 100);
    }

    #[test]
    fn columns_env_unset_uses_fallback() {
        assert_eq!(resolve_width_from(None, || 80), 80);
    }

    #[test]
    fn columns_env_empty_uses_fallback() {
        assert_eq!(resolve_width_from(Some(""), || 80), 80);
    }

    #[test]
    fn columns_env_zero_uses_fallback() {
        assert_eq!(resolve_width_from(Some("0"), || 80), 80);
    }

    #[test]
    fn columns_env_non_numeric_uses_fallback() {
        assert_eq!(resolve_width_from(Some("wide"), || 80), 80);
    }

    #[test]
    fn columns_env_whitespace_trimmed() {
        assert_eq!(resolve_width_from(Some(" 120 "), || 80), 120);
    }
}
