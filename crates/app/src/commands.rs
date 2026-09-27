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

/// Static palette rows: (tag sent back via `Bridge.palette-submit`, label,
/// resolved `Command`). Dynamic rows — `connect` (opens the dialog) and
/// `open-connection:<node-id>` — are appended by `bridge::palette_model`.
pub const PALETTE_COMMANDS: &[(&str, &str, Command)] = &[
    ("execute", "Execute query", Command::ExecuteQuery),
    ("new-query", "New query", Command::NewQuery),
    ("save-query", "Save query", Command::SaveQuery),
    ("search-schema", "Search schema", Command::Search),
    ("open-palette", "Open command palette", Command::OpenPalette),
    ("find", "Find", Command::Find),
    ("search-history", "Search history", Command::SearchHistory),
    ("next-tab", "Next tab", Command::NextTab),
    ("close-tab", "Close tab", Command::CloseTab),
    ("refresh-schema", "Refresh schema", Command::RefreshSchema),
    ("toggle-sidebar", "Toggle sidebar", Command::ToggleSidebar),
];

/// Resolve a static palette tag to its `Command`.
pub fn command_by_tag(tag: &str) -> Option<Command> {
    PALETTE_COMMANDS
        .iter()
        .find(|(t, _, _)| *t == tag)
        .map(|(_, _, c)| *c)
}

/// Case-insensitive subsequence match. `Some(score)` when every char of
/// `needle` appears in `hay` in order — empty needle matches everything.
/// Score rewards contiguity and early first match.
///
/// ponytail: no camel/prefix boosting beyond first-match position; swap in
/// a real fuzzy scorer if ranking ever feels off.
pub fn fuzzy(needle: &str, hay: &str) -> Option<i32> {
    let needle: Vec<char> = needle.to_lowercase().chars().collect();
    let hay: Vec<char> = hay.to_lowercase().chars().collect();
    let mut pos = 0;
    let mut score = 0;
    for &n in &needle {
        let i = hay[pos..].iter().position(|&h| h == n)? + pos;
        score += if i == pos { 2 } else { 1 } - (i as i32 / 16);
        pos = i + 1;
    }
    Some(score)
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

    #[rstest::rstest]
    #[case::empty_matches_all("", "anything", true)]
    #[case::exact("execute", "Execute query", true)]
    #[case::subsequence("eq", "Execute query", true)]
    #[case::case_insensitive("EXQ", "execute query", true)]
    #[case::out_of_order("yq", "Execute query", false)]
    #[case::missing_char("eqz", "Execute query", false)]
    #[case::empty_hay("x", "", false)]
    fn fuzzy_matches(#[case] needle: &str, #[case] hay: &str, #[case] expected: bool) {
        assert_eq!(fuzzy(needle, hay).is_some(), expected);
    }

    #[test]
    fn fuzzy_prefers_contiguous_and_early_matches() {
        // "exe" contiguous beats "e..x..e" scattered.
        assert!(fuzzy("exe", "execute") > fuzzy("exe", "example text"));
        // Earlier first match beats later.
        assert!(fuzzy("q", "query") > fuzzy("q", "aaa query"));
    }

    #[test]
    fn palette_tags_resolve() {
        assert_eq!(command_by_tag("execute"), Some(Command::ExecuteQuery));
        assert_eq!(
            command_by_tag("refresh-schema"),
            Some(Command::RefreshSchema)
        );
        assert_eq!(command_by_tag("connect"), None);
        assert_eq!(command_by_tag("bogus"), None);
    }
}
