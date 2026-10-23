//! The Ishikawa lexer. Port of Pele's `src/diagrams/ishikawa/lexer.ts`.
//!
//! Pele reads UTF-16 code units. This port reads UTF-8 bytes and decodes a non-ASCII character
//! whole wherever `\s` looks at it, so token boundaries match.

use crate::diagrams::common::caseless::word_at;
use crate::util::chars::{is_line_end, is_space, is_word};

/// A token kind. The order mirrors Pele's `T` enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum T {
    /// After the end of input.
    End,
    /// The end of input.
    Eof,
    /// Blank lines, or a comment with the white space before it.
    Spaceline,
    /// One line break before text.
    Nl,
    /// The `ishikawa` or `ishikawa-beta` keyword.
    Ishikawa,
    /// Indentation.
    Spacelist,
    /// The rest of a line.
    Text,
}

impl T {
    /// The name Mermaid's parser reports, from Pele's `TOKEN_NAMES`.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::End => "$end",
            Self::Eof => "EOF",
            Self::Spaceline => "SPACELINE",
            Self::Nl => "NL",
            Self::Ishikawa => "ISHIKAWA",
            Self::Spacelist => "SPACELIST",
            Self::Text => "TEXT",
        }
    }
}

/// A token: its kind and its UTF-8 span.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Token {
    /// End offset.
    pub(crate) end: usize,
    /// Start offset.
    pub(crate) start: usize,
    /// Kind.
    pub(crate) t: T,
}

/// The character at `at`, or `None` at the end.
fn char_at(src: &str, at: usize) -> Option<char> {
    src.get(at..).and_then(|rest| rest.chars().next())
}

/// Whether the character at `at` is a word character. False at the end.
fn word_char_at(src: &str, at: usize) -> bool {
    src.as_bytes()
        .get(at)
        .is_some_and(|&c| is_word(u32::from(c)))
}

/// The length of `ishikawa-beta`, or failing that `ishikawa`, at `at`, either one ending at a word
/// boundary. 0 when neither is there.
fn keyword(src: &str, at: usize) -> usize {
    if word_at(src, at, "ishikawa-beta") && !word_char_at(src, at + 13) {
        return 13;
    }
    if word_at(src, at, "ishikawa") && !word_char_at(src, at + 8) {
        return 8;
    }
    0
}

/// Produces the tokens of Mermaid's Jison lexer for Ishikawa diagrams, where the first rule that
/// matches wins: white space up to a comment or up to the last line break in it is a blank line,
/// a single line break before text is a new line, other white space is indentation, and the rest
/// of a line is text. The list ends with `Eof` and `End`.
pub(crate) fn tokenize(src: &str) -> Vec<Token> {
    let n = src.len();
    let bytes = src.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < n {
        let mut j = i;
        let mut last_break = None;
        while let Some(c) = char_at(src, j).filter(|&c| is_space(u32::from(c))) {
            if c == '\n' {
                last_break = Some(j);
            }
            j += c.len_utf8();
        }

        let (t, end) = if bytes.get(j) == Some(&b'%') && bytes.get(j + 1) == Some(&b'%') {
            let mut end = j + 2;
            while let Some(c) = char_at(src, end).filter(|&c| !is_line_end(u32::from(c))) {
                end += c.len_utf8();
            }
            (T::Spaceline, end)
        } else if j == i {
            match keyword(src, i) {
                0 => (
                    T::Text,
                    src.get(i..)
                        .and_then(|rest| rest.find('\n'))
                        .map_or(n, |at| i + at),
                ),
                length => (T::Ishikawa, i + length),
            }
        } else {
            match last_break {
                Some(at) if at > i => (T::Spaceline, at + 1),
                Some(at) if at == i => (T::Nl, i + 1),
                _ => (T::Spacelist, j),
            }
        };
        tokens.push(Token { end, start: i, t });
        i = end;
    }

    tokens.push(Token {
        end: n,
        start: n,
        t: T::Eof,
    });
    tokens.push(Token {
        end: n,
        start: n,
        t: T::End,
    });
    tokens
}
