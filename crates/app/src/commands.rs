//! Keyboard command mapping (spec §20). Slint sends `KeyEvent` fields
//! (`text`, ctrl/shift/alt) through `Bridge.command`; [`command_for`] is the
//! pure mapping table — the only place a key becomes a [`Command`].
//!
//! `KeyEvent.text` is the *unmodified* key for letters (Ctrl+S arrives as
//! `"s"` with `control = true` — that's how Slint's own `shortcut()`
//! matching works), lowercase-normalized here so Shift+P vs Shift+p doesn't
//! matter. Special keys arrive as Slint key-code chars (`\n` Return, `\t`
//! Tab, `\u{1b}` Escape, `\u{f700}` UpArrow, …).

use datara_domain::Command;

fn is_text(text: &str, ch: char) -> bool {
    text.len() == ch.len_utf8() && text.starts_with(ch)
}

/// Key-combo string like `ctrl+shift+p` — canonical form fed to
/// [`parse_command`] from the `Bridge.command` callback.
pub fn key_string(text: &str, ctrl: bool, shift: bool, alt: bool) -> String {
    let mut s = String::new();
    if ctrl {
        s.push_str("ctrl+");
    }
    if shift {
        s.push_str("shift+");
    }
    if alt {
        s.push_str("alt+");
    }
    s.push_str(&text.to_lowercase());
    s
}

/// Map a Slint key event to a `Command` per spec §20, or `None` when the
/// key isn't an app shortcut (plain typing, navigation, editing chords).
pub fn command_for(text: &str, ctrl: bool, shift: bool, alt: bool) -> Option<Command> {
    if !ctrl || alt {
        return None;
    }
    let lower = text.to_lowercase();
    let cmd = match lower.as_str() {
        t if is_text(t, '\n') => Command::ExecuteQuery,
        "s" if !shift => Command::SaveQuery,
        "k" if !shift => Command::Search,
        "p" => Command::OpenPalette, // ctrl+p and ctrl+shift+p both open it for now
        "f" if !shift => Command::Find,
        "f" => Command::SearchHistory, // ctrl+shift+f
        "n" if !shift => Command::NewQuery,
        "w" if !shift => Command::CloseTab,
        t if is_text(t, '\t') || is_text(t, '\u{19}') => Command::NextTab, // tab / backtab
        _ => return None,
    };
    Some(cmd)
}

/// String form: `"ctrl+shift+p"`, `"ctrl+return"`. Accepts a couple of
/// aliases (`enter`/`escape` names) so the table stays readable.
pub fn parse_command(s: &str) -> Option<Command> {
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut key = String::new();
    for part in s.split('+') {
        let lowered = part.trim().to_lowercase();
        match lowered.as_str() {
            "ctrl" | "control" => ctrl = true,
            "shift" => shift = true,
            "alt" => alt = true,
            _ => key = lowered,
        }
    }
    let text = match key.as_str() {
        "return" | "enter" => "\n",
        "tab" => "\t",
        "escape" | "esc" => "\u{1b}",
        k => k,
    };
    command_for(text, ctrl, shift, alt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    // Spec §20 map.
    #[case::execute("\n", true, false, false, Some(Command::ExecuteQuery))]
    #[case::save("s", true, false, false, Some(Command::SaveQuery))]
    #[case::search("k", true, false, false, Some(Command::Search))]
    #[case::palette("p", true, false, false, Some(Command::OpenPalette))]
    #[case::palette_shift("P", true, true, false, Some(Command::OpenPalette))]
    #[case::find("f", true, false, false, Some(Command::Find))]
    #[case::history("F", true, true, false, Some(Command::SearchHistory))]
    #[case::new("n", true, false, false, Some(Command::NewQuery))]
    #[case::close("w", true, false, false, Some(Command::CloseTab))]
    #[case::next_tab("\t", true, false, false, Some(Command::NextTab))]
    #[case::next_tab_backtab("\u{19}", true, true, false, Some(Command::NextTab))]
    // Non-commands.
    #[case::plain_key("s", false, false, false, None)]
    #[case::bare_return("\n", false, false, false, None)]
    #[case::shift_only("S", false, true, false, None)]
    #[case::alt_blocks("s", true, false, true, None)]
    #[case::ctrl_z_left_to_textinput("z", true, false, false, None)]
    #[case::unmapped("q", true, false, false, None)]
    #[case::arrow_no_mod("\u{f700}", false, false, false, None)]
    fn commands(
        #[case] text: &str,
        #[case] ctrl: bool,
        #[case] shift: bool,
        #[case] alt: bool,
        #[case] expected: Option<Command>,
    ) {
        assert_eq!(command_for(text, ctrl, shift, alt), expected);
    }

    #[rstest::rstest]
    #[case::execute("ctrl+return", Some(Command::ExecuteQuery))]
    #[case::execute_alias("ctrl+enter", Some(Command::ExecuteQuery))]
    #[case::palette("ctrl+p", Some(Command::OpenPalette))]
    #[case::palette_shift("ctrl+shift+p", Some(Command::OpenPalette))]
    #[case::history("ctrl+shift+f", Some(Command::SearchHistory))]
    #[case::next_tab("ctrl+tab", Some(Command::NextTab))]
    #[case::none("p", None)]
    #[case::unmapped("ctrl+alt+q", None)]
    fn parses(#[case] s: &str, #[case] expected: Option<Command>) {
        assert_eq!(parse_command(s), expected);
    }
}
