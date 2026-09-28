use crate::lex::{self, TokKind};

/// Byte-offset range of one SQL statement in the editor buffer.
///
/// Ranges are contiguous and gapless when sorted: whitespace and comments
/// after a `;` fold into the preceding statement — the next statement's
/// range begins at its first non-trivia token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatementRange {
    pub start: usize,
    pub end: usize,
}

/// Split `sql` into statements on top-level `;` tokens.
///
/// A trailing run of text with no terminating `;` becomes the final range.
/// Empty or whitespace/comment-only input yields `[]`.
pub fn split_statements(sql: &str) -> Vec<StatementRange> {
    let toks = lex::scan(sql);
    let mut out = Vec::new();
    let mut start = 0;
    let mut has_real = false;
    // set when a `;` was seen; the range closes at the next non-trivia
    // token so trailing trivia folds into the preceding statement
    let mut pending_close = false;
    for t in &toks {
        if pending_close && !t.is_trivia() {
            out.push(StatementRange {
                start,
                end: t.start,
            });
            start = t.start;
            has_real = false;
            pending_close = false;
        }
        match t.kind {
            // a bare `;` also counts as a statement — only pure
            // whitespace/comment runs produce no range
            TokKind::Semi => {
                has_real = true;
                pending_close = true;
            }
            _ if !t.is_trivia() => has_real = true,
            _ => {}
        }
    }
    // trailing text without ';' is the final statement; a trivia-only tail
    // closes the pending range at EOF
    if has_real || pending_close {
        out.push(StatementRange {
            start,
            end: sql.len(),
        });
    }
    out
}

/// Statement containing `cursor` (a byte offset into `sql`).
///
/// Returns `None` when there is no statement range covering the cursor;
/// callers fall back to the whole document.
pub fn statement_at(sql: &str, cursor: usize) -> Option<StatementRange> {
    let ranges = split_statements(sql);
    for r in &ranges {
        if cursor < r.end {
            if cursor >= r.start {
                return Some(*r);
            }
            return None;
        }
    }
    // cursor at (or past) EOF belongs to the last statement
    ranges.last().copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn ranges(sql: &str) -> Vec<(usize, usize)> {
        split_statements(sql)
            .iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[rstest]
    #[case::single("SELECT 1;", &[(0, 9)])]
    #[case::single_no_semi("SELECT 1", &[(0, 8)])]
    #[case::multi("SELECT 1; SELECT 2;", &[(0, 10), (10, 19)])]
    #[case::semi_in_string("SELECT 'a;b'; SELECT 2", &[(0, 14), (14, 22)])]
    #[case::semi_in_line_comment("SELECT 1 -- x;\nSELECT 2;", &[(0, 24)])]
    #[case::semi_in_block_comment("SELECT 1 /* ; */; SELECT 2", &[(0, 18), (18, 26)])]
    #[case::escaped_quote("SELECT 'it''s;'; SELECT 2", &[(0, 17), (17, 25)])]
    #[case::empty("", &[])]
    #[case::multiline("SELECT 1\nFROM t;\nSELECT 2;", &[(0, 17), (17, 26)])]
    #[case::comments_only("-- nothing\n/* more */", &[])]
    #[case::trailing_trivia("SELECT 1;  -- tail\n", &[(0, 19)])]
    #[case::comment_after_semi("SELECT 1; -- done\nSELECT 2", &[(0, 18), (18, 26)])]
    #[case::no_statements_between_semis("SELECT 1;;SELECT 2", &[(0, 9), (9, 10), (10, 18)])]
    fn split(#[case] sql: &str, #[case] expected: &[(usize, usize)]) {
        assert_eq!(ranges(sql), expected);
    }

    #[test]
    fn statement_at_second_of_three() {
        // ranges: (0,10) (10,20) (20,29); cursor inside 2nd
        assert_eq!(
            statement_at("SELECT 1; SELECT 2; SELECT 3;", 12),
            Some(StatementRange { start: 10, end: 20 })
        );
    }

    #[rstest]
    #[case::in_first(3, StatementRange { start: 0, end: 10 })]
    #[case::cursor_in_folded_space(9, StatementRange { start: 0, end: 10 })]
    #[case::boundary_prefers_next(10, StatementRange { start: 10, end: 19 })]
    #[case::at_eof(19, StatementRange { start: 10, end: 19 })]
    #[case::past_eof(30, StatementRange { start: 10, end: 19 })]
    fn at(#[case] cursor: usize, #[case] expected: StatementRange) {
        assert_eq!(statement_at("SELECT 1; SELECT 2;", cursor), Some(expected));
    }

    #[test]
    fn at_trivia_only_returns_none() {
        assert_eq!(statement_at("", 0), None);
        assert_eq!(statement_at("  -- just a comment\n", 5), None);
    }
}
