use crate::lex::{self, TokKind};
use crate::EXTRA_KEYWORDS;
use sqlparser::keywords::ALL_KEYWORDS;
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Keyword,
    String,
    Number,
    Comment,
    Identifier,
    Operator,
    Punctuation,
    Plain,
}

/// One highlight span in byte offsets; the vector covers the whole input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightToken {
    pub start: usize,
    pub end: usize,
    pub kind: TokenKind,
}

/// Case-insensitive membership test over a sorted uppercase keyword table.
/// Same fold-compare `sqlparser` uses internally; no per-word allocation.
fn in_table(table: &[&str], word: &str) -> bool {
    table
        .binary_search_by(|probe| {
            let probe = probe.as_bytes();
            let word = word.as_bytes();
            for (p, w) in probe.iter().zip(word.iter()) {
                let cmp = p.cmp(&w.to_ascii_uppercase());
                if cmp != Ordering::Equal {
                    return cmp;
                }
            }
            probe.len().cmp(&word.len())
        })
        .is_ok()
}

/// `sqlparser`'s `GenericDialect` keyword set plus MSSQL extras that the
/// parser doesn't model (DATETIME2, ISNULL, GETDATE, ...).
fn is_keyword(word: &str) -> bool {
    in_table(ALL_KEYWORDS, word) || in_table(EXTRA_KEYWORDS, word)
}

/// Tokenize `sql` into highlight spans with exact byte offsets.
/// Output covers the entire input; whitespace becomes `Plain`.
pub fn highlight(sql: &str) -> Vec<HighlightToken> {
    lex::scan(sql)
        .into_iter()
        .map(|t| {
            let kind = match t.kind {
                TokKind::Word => {
                    let text = &sql[t.start..t.end];
                    // @var / #tmp / $x are never keywords
                    if !text.starts_with(['@', '#', '$']) && is_keyword(text) {
                        TokenKind::Keyword
                    } else {
                        TokenKind::Identifier
                    }
                }
                TokKind::Str => TokenKind::String,
                TokKind::QuotedIdent => TokenKind::Identifier,
                TokKind::Number => TokenKind::Number,
                TokKind::Comment => TokenKind::Comment,
                TokKind::Semi | TokKind::Punct => TokenKind::Punctuation,
                TokKind::Op => TokenKind::Operator,
                TokKind::Other => TokenKind::Plain,
            };
            HighlightToken {
                start: t.start,
                end: t.end,
                kind,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind_at(sql: &str, needle: &str) -> TokenKind {
        let at = sql.find(needle).unwrap();
        let binding = highlight(sql);
        let tok = binding
            .iter()
            .find(|t| at >= t.start && at < t.end)
            .unwrap();
        assert_eq!(&sql[tok.start..tok.end], needle);
        tok.kind
    }

    #[rstest::rstest]
    #[case::keyword("SELECT 1", "SELECT", TokenKind::Keyword)]
    #[case::keyword_case_insensitive("select 1", "select", TokenKind::Keyword)]
    #[case::mssql_keyword("SELECT TOP 1", "TOP", TokenKind::Keyword)]
    #[case::mssql_type("CAST(x AS NVARCHAR(10))", "NVARCHAR", TokenKind::Keyword)]
    #[case::mssql_missing("SELECT GETDATE()", "GETDATE", TokenKind::Keyword)]
    #[case::string("SELECT 'x'", "'x'", TokenKind::String)]
    #[case::nstring("SELECT N'x'", "N'x'", TokenKind::String)]
    #[case::line_comment("-- note", "-- note", TokenKind::Comment)]
    #[case::block_comment("/* note */", "/* note */", TokenKind::Comment)]
    #[case::number("SELECT 1.5", "1.5", TokenKind::Number)]
    #[case::ident("SELECT col1", "col1", TokenKind::Identifier)]
    #[case::operator("a >= b", ">", TokenKind::Operator)]
    #[case::paren("f(x)", "(", TokenKind::Punctuation)]
    #[case::semi("SELECT 1;", ";", TokenKind::Punctuation)]
    #[case::whitespace("a b", " ", TokenKind::Plain)]
    #[case::bracket_ident("[dbo].[t]", "[dbo]", TokenKind::Identifier)]
    #[case::dquoted_ident("SELECT \"x\"", "\"x\"", TokenKind::Identifier)]
    #[case::variable("@n", "@n", TokenKind::Identifier)]
    fn classify(#[case] sql: &str, #[case] needle: &str, #[case] expected: TokenKind) {
        assert_eq!(kind_at(sql, needle), expected);
    }

    #[test]
    fn coverage_is_contiguous() {
        // multibyte char inside a string: offsets must stay byte-exact
        let sql = "SELECT 'héllo', t.[c] -- x\nFROM [dbo].[t] WHERE a >= 1.5e2;";
        let toks = highlight(sql);
        assert_eq!(toks.first().unwrap().start, 0);
        assert_eq!(toks.last().unwrap().end, sql.len());
        for w in toks.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
    }

    #[test]
    fn unterminated_string_does_not_panic() {
        let sql = "SELECT 'open";
        let toks = highlight(sql);
        let last = toks.last().unwrap();
        assert_eq!(last.kind, TokenKind::String);
        assert_eq!(last.end, sql.len());
    }
}
