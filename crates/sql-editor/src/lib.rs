//! Datara sql-editor crate.
//!
//! Byte-offset SQL lexing for the editor: statement splitting, highlight
//! tokenization and keyword/catalog completions. Uses a small hand-rolled
//! scanner rather than `sqlparser`'s `Tokenizer` because sqlparser reports
//! `Location` (line/column) spans instead of byte offsets and fails hard on
//! unterminated literals, which are the common case while typing.

mod complete;
mod highlight;
mod lex;
mod statement;

pub use complete::completions;
pub use highlight::{highlight, HighlightToken, TokenKind};
pub use statement::{split_statements, statement_at, StatementRange};

/// T-SQL / MSSQL keywords that `sqlparser::keywords::ALL_KEYWORDS` misses.
/// Sorted; used for highlighting and merged into completion candidates.
pub const EXTRA_KEYWORDS: &[&str] = &[
    "BIGINT",
    "COALESCE",
    "DATETIME2",
    "DATETIMEOFFSET",
    "GETDATE",
    "IDENTITY",
    "ISNULL",
    "NCHAR",
    "NVARCHAR",
    "SMALLINT",
    "SYSDATETIME",
    "TINYINT",
    "TOP",
    "UNIQUEIDENTIFIER",
];
