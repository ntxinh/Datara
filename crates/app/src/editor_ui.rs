//! Editor state living on the Rust side: tab list (the UI has a single
//! editor instance; each tab's text/cursor stash here), completion popup
//! bookkeeping, and the `highlight` tokens → `HighlightSpan` cells mapping.
//!
//! Everything in here is UI-thread state — `EditorState` moves through
//! `UiHandle` behind the same `Arc<Mutex<_>>` pattern as `SchemaTree`.

use datara_domain::ConnectionId;
use datara_sql_editor::{completions, highlight, statement_at, TokenKind};

use crate::HighlightSpan;

/// One editor tab. `conn_id`/`database` are set when the tab was opened
/// from a schema-tree action; `None` falls back to `Backend::last_conn_id`.
/// `last_query` is the SQL most recently sent from this tab (Ctrl+Enter).
#[derive(Debug, Clone)]
pub struct EditorTab {
    pub id: i32,
    pub title: String,
    pub text: String,
    pub cursor: usize,
    pub conn_id: Option<ConnectionId>,
    pub database: Option<String>,
    pub last_query: Option<String>,
}

/// Tab strip + active buffer + completion popup state.
#[derive(Debug)]
pub struct EditorState {
    pub tabs: Vec<EditorTab>,
    /// Index into `tabs` — always valid (a state never has zero tabs).
    pub active: usize,
    /// Visible completion candidates; empty = popup closed.
    pub completions: Vec<String>,
    /// Selected row in the popup.
    pub completion_index: usize,
    /// Caret byte offset in the active buffer (mirrors TextInput).
    pub cursor: usize,
    /// Selection anchor byte offset; `anchor != cursor` means a selection.
    pub anchor: usize,
    /// Word range the current `completions` were computed for.
    completion_range: (usize, usize),
    next_tab_id: i32,
    untitled: u32,
    /// Rolling nonce packed into `Bridge.set-cursor` (offset<<8 | nonce) so
    /// repeated identical offsets still re-trigger the editor watcher.
    cursor_nonce: u8,
}

impl Default for EditorState {
    fn default() -> Self {
        let mut s = Self {
            tabs: Vec::new(),
            active: 0,
            completions: Vec::new(),
            completion_index: 0,
            cursor: 0,
            anchor: 0,
            completion_range: (0, 0),
            next_tab_id: 0,
            untitled: 0,
            cursor_nonce: 0,
        };
        s.new_tab();
        s
    }
}

impl EditorState {
    pub fn active_tab(&self) -> &EditorTab {
        &self.tabs[self.active]
    }

    /// `Bridge.set-cursor` payload: byte offset plus a nonce that defeats
    /// property-equality when the same offset is sent twice.
    pub fn cursor_jump(&mut self, offset: usize) -> i32 {
        self.cursor_nonce = self.cursor_nonce.wrapping_add(1);
        ((offset.min((1 << 23) - 1) as i32) << 8) | i32::from(self.cursor_nonce)
    }

    fn fresh_id(&mut self) -> i32 {
        self.next_tab_id += 1;
        self.next_tab_id
    }

    /// Mirror the editor's caret/selection byte offsets (cursor-changed).
    pub fn set_caret(&mut self, cursor: usize, anchor: usize) {
        self.cursor = cursor;
        self.anchor = anchor;
    }

    /// Save the live caret into the outgoing tab — caret moves don't go
    /// through `stash`, so every activation change must do this or the
    /// position is lost.
    fn stash_cursor(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.cursor = self.cursor;
        }
    }

    /// Open a blank "Query N" tab and make it active.
    pub fn new_tab(&mut self) -> i32 {
        self.untitled += 1;
        self.stash_cursor();
        let id = self.fresh_id();
        self.tabs.push(EditorTab {
            id,
            title: format!("Query {}", self.untitled),
            text: String::new(),
            cursor: 0,
            conn_id: None,
            database: None,
            last_query: None,
        });
        self.active = self.tabs.len() - 1;
        self.completions.clear();
        self.cursor = 0;
        self.anchor = 0;
        id
    }

    /// Open a tab carrying SQL — used by `PreviewSql` (double-click on a
    /// table). Returns the new tab id.
    pub fn open_sql_tab(
        &mut self,
        title: String,
        text: String,
        conn_id: Option<ConnectionId>,
        database: Option<String>,
    ) -> i32 {
        self.stash_cursor();
        let id = self.fresh_id();
        self.tabs.push(EditorTab {
            id,
            title,
            text,
            cursor: 0,
            conn_id,
            database,
            last_query: None,
        });
        self.active = self.tabs.len() - 1;
        self.completions.clear();
        self.cursor = 0;
        self.anchor = 0;
        id
    }

    /// Rebuild tabs from persisted `[workspace] open_tabs` (each entry is
    /// a tab's SQL text). Empty → the default single blank tab.
    pub fn restore(&mut self, texts: &[String]) {
        if texts.is_empty() {
            return;
        }
        self.tabs.clear();
        for (i, text) in texts.iter().enumerate() {
            self.open_sql_tab(format!("Query {}", i + 1), text.clone(), None, None);
        }
        self.untitled = texts.len() as u32;
        // Back on the first tab — the active index isn't persisted.
        self.active = 0;
        self.cursor = 0;
        self.anchor = 0;
    }

    /// Persist the live editor text/caret into the active tab — called
    /// before any switch/close/execute so `tabs[active]` is current.
    pub fn stash(&mut self, text: String, cursor: usize) {
        let tab = &mut self.tabs[self.active];
        tab.text = text;
        tab.cursor = cursor;
    }

    /// Switch to tab `id`: store the outgoing buffer, return `(text,
    /// cursor)` of the incoming tab for the UI to load. Unknown id → None.
    pub fn switch(&mut self, id: i32, outgoing_text: String) -> Option<(String, usize)> {
        let idx = self.tabs.iter().position(|t| t.id == id)?;
        let out = &mut self.tabs[self.active];
        out.text = outgoing_text;
        out.cursor = self.cursor;
        self.active = idx;
        self.completions.clear();
        self.cursor = self.tabs[idx].cursor.min(self.tabs[idx].text.len());
        self.anchor = self.cursor;
        let tab = &self.tabs[idx];
        Some((tab.text.clone(), tab.cursor))
    }

    /// Close tab `id`; returns `(text, cursor)` to load into the editor for
    /// the tab that takes over, or None when `id` isn't open. Closing the
    /// last tab opens a fresh "Query N" so `active` is never invalid.
    pub fn close(&mut self, id: i32, outgoing_text: String) -> Option<(String, usize)> {
        let idx = self.tabs.iter().position(|t| t.id == id)?;
        let out = &mut self.tabs[self.active];
        out.text = outgoing_text;
        out.cursor = self.cursor;
        self.tabs.remove(idx);
        if self.tabs.is_empty() {
            self.new_tab();
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if idx < self.active {
            self.active -= 1;
        }
        self.completions.clear();
        self.cursor = self.active_tab().cursor.min(self.active_tab().text.len());
        self.anchor = self.cursor;
        let tab = self.active_tab();
        Some((tab.text.clone(), tab.cursor))
    }

    /// Activate the tab after the active one (wraps). Returns
    /// `(id, text, cursor)` to load, or None for a single-tab state.
    pub fn next_tab(&mut self, outgoing_text: String) -> Option<(i32, String, usize)> {
        if self.tabs.len() < 2 {
            return None;
        }
        let out = &mut self.tabs[self.active];
        out.text = outgoing_text;
        out.cursor = self.cursor;
        self.active = (self.active + 1) % self.tabs.len();
        self.completions.clear();
        self.cursor = self.active_tab().cursor.min(self.active_tab().text.len());
        self.anchor = self.cursor;
        let tab = self.active_tab();
        Some((tab.id, tab.text.clone(), tab.cursor))
    }

    /// Refresh the completion list for `text`/`cursor` (the word prefix
    /// ending at the caret) against `catalog` (schema labels). Hides the
    /// popup for an empty prefix or no matches.
    pub fn update_completions(&mut self, text: &str, cursor: usize, catalog: &[String]) {
        let Some((start, end)) = word_range(text, cursor) else {
            self.completions.clear();
            return;
        };
        let prefix = &text[start..end];
        let found = completions(prefix, catalog);
        if found.is_empty() {
            self.completions.clear();
            return;
        }
        self.completions = found;
        self.completion_index = 0;
        self.completion_range = (start, end);
    }

    /// Accept the selected completion: replace the recorded word range in
    /// `text`, returning `(new_text, new_cursor)`; None when the popup is
    /// closed or the buffer moved on since the range was computed.
    pub fn apply_completion(&mut self, text: &str) -> Option<(String, usize)> {
        let picked = self.completions.get(self.completion_index)?.clone();
        self.completions.clear();
        let (start, end) = self.completion_range;
        if end > text.len() || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return None;
        }
        let mut new_text = String::with_capacity(text.len() + picked.len());
        new_text.push_str(&text[..start]);
        new_text.push_str(&picked);
        new_text.push_str(&text[end..]);
        Some((new_text, start + picked.len()))
    }

    /// Move the popup selection by `delta` (wraps).
    pub fn move_completion(&mut self, delta: isize) {
        let n = self.completions.len() as isize;
        if n == 0 {
            return;
        }
        self.completion_index = ((self.completion_index as isize + delta).rem_euclid(n)) as usize;
    }
}

/// SQL to execute for `Command::ExecuteQuery`: the selection when there is
/// one, else the statement under the cursor, else the whole document.
/// `statement_at` returns None only for leading trivia before the first
/// statement (a trivia-only buffer) — running the whole doc then matches
/// other SQL clients. Empty buffer → None.
pub fn resolve_sql(text: &str, anchor: usize, cursor: usize) -> Option<(usize, usize)> {
    let (lo, hi) = (anchor.min(cursor), anchor.max(cursor));
    if lo != hi && hi <= text.len() && text.is_char_boundary(lo) && text.is_char_boundary(hi) {
        return Some((lo, hi));
    }
    let cursor = cursor.min(text.len());
    match statement_at(text, cursor).map(|r| (r.start, r.end)) {
        Some(range) => Some(range),
        None => (!text.is_empty()).then_some((0, text.len())),
    }
}

/// `[A-Za-z0-9_]` run ending at `cursor` — the completion prefix. None for
/// an empty word so the popup doesn't open on every keystroke.
fn word_range(text: &str, cursor: usize) -> Option<(usize, usize)> {
    let cursor = cursor.min(text.len());
    if !text.is_char_boundary(cursor) {
        return None;
    }
    let start = text[..cursor]
        .char_indices()
        .rev()
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_'))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    (start != cursor).then_some((start, cursor))
}

/// `highlight()` tokens → per-line `HighlightSpan`s in char cells.
///
/// `Plain` spans are skipped: the `TextInput` already paints its whole
/// text in the foreground color, so only colored tokens need an overlay
/// glyph. Multi-line tokens (block comments, literals) are split per line;
/// each piece's `col` counts *chars* from its line start.
pub fn highlight_spans(text: &str) -> Vec<HighlightSpan> {
    let mut spans = Vec::new();
    let mut line = 0i32;
    let mut line_start = 0usize;
    for tok in highlight(text) {
        let mut seg = tok.start;
        while seg <= tok.end {
            let nl = text[seg..tok.end].find('\n').map(|i| seg + i);
            let piece_end = nl.unwrap_or(tok.end);
            if tok.kind != TokenKind::Plain && piece_end > seg {
                spans.push(HighlightSpan {
                    text: text[seg..piece_end].into(),
                    color: span_color(tok.kind),
                    line,
                    col: text[line_start..seg].chars().count() as i32,
                });
            }
            match nl {
                Some(i) => {
                    line += 1;
                    line_start = i + 1;
                    seg = i + 1;
                }
                None => break,
            }
        }
    }
    spans
}

fn span_color(kind: TokenKind) -> slint::Color {
    // Catppuccin Mocha accents.
    match kind {
        TokenKind::Keyword => slint::Color::from_rgb_u8(0xCB, 0xA6, 0xF7), // mauve
        TokenKind::String => slint::Color::from_rgb_u8(0xA6, 0xE3, 0xA1),  // green
        TokenKind::Number => slint::Color::from_rgb_u8(0xFA, 0xB3, 0x87),  // peach
        TokenKind::Comment => slint::Color::from_rgb_u8(0x6C, 0x70, 0x86), // overlay0
        TokenKind::Identifier => slint::Color::from_rgb_u8(0x89, 0xB4, 0xFA), // blue
        TokenKind::Operator => slint::Color::from_rgb_u8(0x89, 0xDC, 0xEB), // sky
        TokenKind::Punctuation => slint::Color::from_rgb_u8(0xBA, 0xC2, 0xDE), // subtext1
        TokenKind::Plain => slint::Color::from_rgb_u8(0xCD, 0xD6, 0xF4),   // text
    }
}

/// `Ln X, Col Y` (1-based, col in chars) — Slint computes its own copy from
/// caret pixels; this is the canonical byte→position for tests/logging.
pub fn line_col(text: &str, byte: usize) -> (usize, usize) {
    let byte = byte.min(text.len());
    let line = text[..byte].bytes().filter(|b| *b == b'\n').count() + 1;
    let col = text[..byte]
        .rsplit('\n')
        .next()
        .map(|s| s.chars().count())
        .unwrap_or(0)
        + 1;
    (line, col)
}

/// Displayed line count — `split` counts the empty line after a trailing
/// newline, matching what the editor renders.
pub fn line_count(text: &str) -> usize {
    text.split('\n').count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> EditorState {
        EditorState::default()
    }

    #[test]
    fn switch_saves_and_restores_text() {
        let mut s = state();
        let first = s.active_tab().id;
        s.stash("SELECT 1".into(), 0);
        s.new_tab(); // active = second tab, empty
        s.stash("SELECT 2".into(), 0);

        // Switch back to first: its text comes back.
        let (text, _cursor) = s.switch(first, "SELECT 2".into()).unwrap();
        assert_eq!(text, "SELECT 1");
        assert_eq!(s.active_tab().id, first);

        // And the second tab kept its own text.
        let second = s.tabs.iter().find(|t| t.id != first).unwrap().id;
        let (text, _) = s.switch(second, "SELECT 1".into()).unwrap();
        assert_eq!(text, "SELECT 2");
    }

    #[test]
    fn close_active_moves_to_neighbor() {
        let mut s = state();
        let first = s.active_tab().id;
        s.stash("a".into(), 0);
        s.new_tab();
        let second = s.active_tab().id;

        let (text, _) = s.close(second, "b".into()).unwrap();
        assert_eq!(s.active_tab().id, first);
        assert_eq!(text, "a");
        assert_eq!(s.tabs.len(), 1);
    }

    #[test]
    fn close_last_tab_opens_fresh() {
        let mut s = state();
        let only = s.active_tab().id;
        let (text, _) = s.close(only, "x".into()).unwrap();
        assert_eq!(s.tabs.len(), 1);
        assert_eq!(text, "");
    }

    #[test]
    fn switch_restores_caret() {
        let mut s = state();
        let first = s.active_tab().id;
        s.stash("SELECT 1".into(), 0);
        // Caret moved without editing, then switch away and back.
        s.set_caret(4, 4);
        s.new_tab();
        let (_, cursor) = s.switch(first, "".into()).unwrap();
        assert_eq!(cursor, 4, "caret must round-trip through the tab");
        assert_eq!(s.cursor, 4);
    }

    #[test]
    fn next_tab_wraps() {
        let mut s = state();
        let first = s.active_tab().id;
        s.new_tab();
        let (id, _, _) = s.next_tab("".into()).unwrap();
        assert_eq!(id, first);
        let second = s.tabs[1].id;
        assert_eq!(s.next_tab("".into()).unwrap().0, second);
    }

    #[test]
    fn single_tab_next_is_none() {
        let mut s = state();
        assert!(s.next_tab("x".into()).is_none());
    }

    #[rstest::rstest]
    #[case::selection("SELECT 1; SELECT 2", 0, 8, Some((0, 8)))]
    #[case::cursor_in_stmt("SELECT 1; SELECT 2", 12, 12, Some((10, 18)))]
    #[case::cursor_first("SELECT 1; SELECT 2", 3, 3, Some((0, 10)))]
    #[case::empty("", 0, 0, None)]
    // Cursor in leading trivia before the first statement: no covering
    // range → whole doc. (Trailing trivia folds into the previous stmt.)
    #[case::leading_trivia("   SELECT 1", 1, 1, Some((0, 11)))]
    // Trivia-only buffer: nothing to run as a statement → whole doc.
    #[case::trivia_only("-- x\n", 0, 0, Some((0, 5)))]
    // Collapsed selection (anchor == cursor) → falls through to statement.
    #[case::collapsed_selection("SELECT 1; SELECT 2", 8, 8, Some((0, 10)))]
    // Selection ending mid-char ('€' spans bytes 8..11) is rejected →
    // statement mode instead of a half-sliced UTF-8 range.
    #[case::mid_char_selection("SELECT '€'", 0, 10, Some((0, 12)))]
    fn resolves(
        #[case] text: &str,
        #[case] anchor: usize,
        #[case] cursor: usize,
        #[case] expected: Option<(usize, usize)>,
    ) {
        assert_eq!(resolve_sql(text, anchor, cursor), expected);
    }

    #[rstest::rstest]
    #[case::mid_word("SELECT x", 6, Some((0, 6)))]
    #[case::after_space("SELECT x", 7, None)]
    #[case::underscores("foo_bar ", 7, Some((0, 7)))]
    #[case::at_start("x", 0, None)]
    fn word_ranges(
        #[case] text: &str,
        #[case] cursor: usize,
        #[case] expected: Option<(usize, usize)>,
    ) {
        assert_eq!(word_range(text, cursor), expected);
    }

    #[test]
    fn completion_apply_replaces_word() {
        let mut s = state();
        s.update_completions("SEL", 3, &[]);
        assert!(s.completions.iter().any(|c| c == "SELECT"));
        s.completion_index = s.completions.iter().position(|c| c == "SELECT").unwrap();
        let (text, cursor) = s.apply_completion("SEL").unwrap();
        assert_eq!(text, "SELECT");
        assert_eq!(cursor, 6);
        assert!(s.completions.is_empty());
    }

    #[test]
    fn completion_apply_rejects_stale_range() {
        let mut s = state();
        s.update_completions("SEL", 3, &[]);
        // Buffer shrank since the popup was computed → drop, don't corrupt.
        assert!(s.apply_completion("S").is_none());
    }

    #[test]
    fn completion_nav_wraps() {
        let mut s = state();
        s.completions = vec!["a".into(), "b".into()];
        s.completion_index = 0;
        s.move_completion(-1);
        assert_eq!(s.completion_index, 1);
        s.move_completion(1);
        assert_eq!(s.completion_index, 0);
    }

    #[test]
    fn highlight_emits_colored_per_line_spans() {
        let spans = highlight_spans("-- hi\nSELECT 1");
        let comment = spans.iter().find(|s| s.line == 0 && s.text == "-- hi");
        let keyword = spans.iter().find(|s| s.line == 1 && s.text == "SELECT");
        assert!(comment.is_some(), "comment span on line 0: {spans:?}");
        assert!(keyword.is_some(), "keyword span on line 1: {spans:?}");
        assert_eq!(keyword.unwrap().col, 0);
        // Whitespace/Plain tokens produce no overlay glyphs.
        assert!(spans.iter().all(|s| !s.text.trim().is_empty()));
    }

    #[test]
    fn highlight_col_counts_chars_after_leading_space() {
        let spans = highlight_spans("  SELECT");
        let kw = spans.iter().find(|s| s.text == "SELECT").unwrap();
        assert_eq!(kw.col, 2);
    }

    #[rstest::rstest]
    #[case::start("ab\ncd", 0, (1, 1))]
    #[case::second_line("ab\ncd", 4, (2, 2))]
    #[case::eof_nl("a\n", 2, (2, 1))]
    fn positions(#[case] text: &str, #[case] byte: usize, #[case] expected: (usize, usize)) {
        assert_eq!(line_col(text, byte), expected);
    }

    #[rstest::rstest]
    #[case::empty("", 1)]
    #[case::one("a", 1)]
    #[case::trailing_nl("a\n", 2)]
    #[case::multi("a\nb\nc", 3)]
    fn counts_lines(#[case] text: &str, #[case] expected: usize) {
        assert_eq!(line_count(text), expected);
    }
}
