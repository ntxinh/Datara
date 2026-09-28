//! Minimal byte-offset SQL lexer shared by statement splitting and
//! highlighting. Token boundaries always fall on UTF-8 char boundaries.
//!
//! Deliberately hand-rolled: `sqlparser`'s tokenizer yields `Location`
//! (line/char-column) spans rather than byte offsets and errors out on
//! unterminated literals/comments — normal while a user is typing. This
//! scanner never fails and always covers the whole input.

/// A raw lexed token. `Word` covers identifiers/keywords; classification
/// happens in `highlight`. `Other` is whitespace and anything else.
#[derive(Debug)]
pub struct Tok {
    pub start: usize,
    pub end: usize,
    pub kind: TokKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokKind {
    Word,
    Number,
    Str,         // 'x', N'x' string literals
    QuotedIdent, // "x" (ANSI quoted ident), [x] bracketed ident
    Comment,     // -- to EOL or /* */
    Semi,
    Punct, // , ( ) . : etc.
    Op,    // = < > + - * / % ! ~ ^ & |
    Other, // whitespace and unclassifiable bytes
}

impl Tok {
    /// Whitespace/comments — folded into the preceding statement.
    pub fn is_trivia(&self) -> bool {
        matches!(self.kind, TokKind::Other | TokKind::Comment)
    }
}

fn is_word_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || matches!(b, b'_' | b'@' | b'#' | b'$') || b >= 0x80
}

fn is_word_char(b: u8) -> bool {
    is_word_start(b) || b.is_ascii_digit()
}

/// Scan a quote-delimited run starting at `i` (index of the opening
/// delimiter). `close` doubles as its own escape ('' / "" / ]]> in brackets).
/// Unterminated runs extend to EOF.
fn quoted(bytes: &[u8], mut i: usize, close: u8) -> usize {
    i += 1;
    while i < bytes.len() {
        if bytes[i] == close {
            i += 1;
            if i < bytes.len() && bytes[i] == close {
                i += 1; // escaped '' / "" / ]]
                continue;
            }
            break;
        }
        i += 1;
    }
    i
}

pub fn scan(sql: &str) -> Vec<Tok> {
    let b = sql.as_bytes();
    let n = b.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let start = i;
        // T-SQL Unicode literal N'...'
        if (b[i] == b'N' || b[i] == b'n') && i + 1 < n && b[i + 1] == b'\'' {
            let end = quoted(b, i + 1, b'\'');
            out.push(Tok {
                start,
                end,
                kind: TokKind::Str,
            });
            i = end;
            continue;
        }
        let kind = match b[i] {
            c if c.is_ascii_whitespace() => {
                while i < n && b[i].is_ascii_whitespace() {
                    i += 1;
                }
                TokKind::Other
            }
            b'-' if i + 1 < n && b[i + 1] == b'-' => {
                while i < n && b[i] != b'\n' {
                    i += 1;
                }
                TokKind::Comment
            }
            b'/' if i + 1 < n && b[i + 1] == b'*' => {
                i += 2;
                while i < n && !(b[i] == b'*' && i + 1 < n && b[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(n);
                TokKind::Comment
            }
            b'\'' => {
                i = quoted(b, i, b'\'');
                TokKind::Str
            }
            b'"' | b'[' => {
                let close = if b[i] == b'"' { b'"' } else { b']' };
                i = quoted(b, i, close);
                TokKind::QuotedIdent
            }
            c if c.is_ascii_digit() => {
                while i < n && b[i].is_ascii_digit() {
                    i += 1;
                }
                if i < n && b[i] == b'.' {
                    i += 1;
                    while i < n && b[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                // optional exponent: 1e5, 1.5e-3 (only if digits follow)
                if i < n && (b[i] == b'e' || b[i] == b'E') {
                    let mut j = i + 1;
                    if j < n && (b[j] == b'+' || b[j] == b'-') {
                        j += 1;
                    }
                    if j < n && b[j].is_ascii_digit() {
                        i = j;
                        while i < n && b[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
                TokKind::Number
            }
            c if is_word_start(c) => {
                i += 1;
                while i < n && is_word_char(b[i]) {
                    i += 1;
                }
                TokKind::Word
            }
            b';' => {
                i += 1;
                TokKind::Semi
            }
            b'(' | b')' | b',' | b'.' | b':' => {
                i += 1;
                TokKind::Punct
            }
            b'=' | b'<' | b'>' | b'+' | b'-' | b'*' | b'/' | b'%' | b'!' | b'~' | b'^' | b'&'
            | b'|' => {
                i += 1;
                TokKind::Op
            }
            _ => {
                i += 1;
                TokKind::Other
            }
        };
        out.push(Tok {
            start,
            end: i,
            kind,
        });
    }
    out
}
