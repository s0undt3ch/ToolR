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
}
