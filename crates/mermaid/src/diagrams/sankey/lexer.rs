//! The sankey lexer: Mermaid's CSV grammar for sankey rows. Port of Pele's
//! `src/diagrams/sankey/lexer.ts`.
//!
//! Pele reads UTF-16 code units. This port reads UTF-8 bytes: every token but the keyword is
//! printable ASCII, so any other character stops the lexer at the same place.

use crate::util::chars::is_word;

/// A token kind. Variant order mirrors Pele's `T` enum and indexes [`NAMES`], so it is observable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum T {
    /// The end of input.
    End,
    /// Not a Jison token: the reference lexer would return the same empty token forever from here.
    Stuck,
    /// `sankey` or `sankey-beta`.
    Sankey,
    /// A line break.
    Newline,
    /// A field separator.
    Comma,
    /// A double quote around an escaped field.
    Dquote,
    /// An unquoted field.
    NonEscapedText,
    /// The text of a quoted field.
    EscapedText,
    /// The end of the CSV.
    Eof,
}

/// The names Mermaid's parser reports for each token kind, indexed by [`T`].
pub(crate) const NAMES: [&str; 9] = [
    "$end",
    "STUCK",
    "SANKEY",
    "NEWLINE",
    "COMMA",
    "DQUOTE",
    "NON_ESCAPED_TEXT",
    "ESCAPED_TEXT",
    "EOF",
];

impl T {
    /// The name Mermaid's parser reports for this kind.
    pub(crate) fn name(self) -> &'static str {
        NAMES.get(self as usize).copied().unwrap_or("")
    }
}

/// A token: its kind, its start as a UTF-8 offset, and its text.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Token<'a> {
    /// The token kind.
    pub(crate) kind: T,
    /// The UTF-8 offset of the token in the source.
    pub(crate) start: usize,
    /// The matched text.
    pub(crate) text: &'a str,
}

/// Lexer start conditions, as in Mermaid's grammar.
#[derive(Clone, Copy, PartialEq, Eq)]
enum S {
    Csv,
    EscapedText,
    Initial,
}

/// RFC 4180 TEXTDATA: printable ASCII except the double quote and the comma.
fn is_text_data(c: u8) -> bool {
    (32..=126).contains(&c) && c != b'"' && c != b','
}

/// The end of `word` when the source at `p` spells it in any case and no word character follows,
/// as `src.slice(p, p + word.length).toLowerCase() === word` tests it. The Kelvin sign is the one
/// non-ASCII character JavaScript lowercases to a letter of these keywords.
fn keyword(src: &str, p: usize, word: &str) -> Option<usize> {
    let mut chars = src.get(p..)?.char_indices();
    for expected in word.bytes() {
        let (_, c) = chars.next()?;
        let lower = if c == '\u{212a}' {
            'k'
        } else {
            c.to_ascii_lowercase()
        };
        if u32::from(lower) != u32::from(expected) {
            return None;
        }
    }
    let end = chars.next().map_or(src.len(), |(at, _)| p + at);
    let next = src.get(end..).and_then(|rest| rest.chars().next());
    if next.is_some_and(|c| is_word(u32::from(c))) {
        return None;
    }
    Some(end)
}

/// Splits prepared sankey text into tokens. The list ends with [`T::End`], or with [`T::Stuck`]
/// where no rule consumes a character.
pub(crate) fn tokenize<'a>(src: &'a str) -> Vec<Token<'a>> {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut tokens = Vec::new();
    let mut stack: Vec<S> = Vec::new();
    let mut state = S::Initial;
    let mut p = 0;

    let emit = |tokens: &mut Vec<Token<'a>>, kind: T, s: usize, e: usize| -> usize {
        tokens.push(Token {
            kind,
            start: s,
            text: src.get(s..e).unwrap_or(""),
        });
        e
    };
    let at = |i: usize| bytes.get(i).copied();

    while p < n {
        let Some(c) = at(p) else {
            break;
        };
        let start = p;
        let mut q = p;
        if state == S::EscapedText {
            if c == b'"' && at(p + 1) != Some(b'"') {
                if let Some(previous) = stack.pop() {
                    state = previous;
                }
                p = emit(&mut tokens, T::Dquote, p, p + 1);
                continue;
            }
            while let Some(d) = at(q) {
                if is_text_data(d) || d == b',' || d == b'\r' || d == b'\n' {
                    q += 1;
                } else if d == b'"' && at(q + 1) == Some(b'"') {
                    q += 2;
                } else {
                    break;
                }
            }
            p = emit(&mut tokens, T::EscapedText, p, q);
        } else {
            if state == S::Initial && (c | 32) == b's' {
                let end = keyword(src, p, "sankey-beta").or_else(|| keyword(src, p, "sankey"));
                if let Some(end) = end {
                    stack.push(state);
                    state = S::Csv;
                    p = emit(&mut tokens, T::Sankey, p, end);
                    continue;
                }
            }
            if c == b'\n' || (c == b'\r' && at(p + 1) == Some(b'\n')) {
                let end = if c == b'\n' { p + 1 } else { p + 2 };
                p = emit(&mut tokens, T::Newline, p, end);
                continue;
            }
            if c == b',' {
                p = emit(&mut tokens, T::Comma, p, p + 1);
                continue;
            }
            if c == b'"' {
                stack.push(state);
                state = S::EscapedText;
                p = emit(&mut tokens, T::Dquote, p, p + 1);
                continue;
            }
            while at(q).is_some_and(is_text_data) {
                q += 1;
            }
            p = emit(&mut tokens, T::NonEscapedText, p, q);
        }
        // A character outside the CSV alphabet matches as empty text and is never consumed.
        if q == start {
            emit(&mut tokens, T::Stuck, p, p);
            return tokens;
        }
    }

    let last = if state == S::EscapedText {
        T::EscapedText
    } else {
        T::Eof
    };
    emit(&mut tokens, last, n, n);
    emit(&mut tokens, T::End, n, n);
    tokens
}
