//! The venn lexer, which reads tokens one at a time as Mermaid's Jison lexer does: every rule
//! ignores case, the first rule that matches wins, and a keyword must end at a word boundary.
//! Port of Pele's `src/diagrams/venn/lexer.ts`.
//!
//! Pele reads UTF-16 code units. This port reads UTF-8 bytes: every delimiter is ASCII, and a
//! non-ASCII character is decoded whole wherever `\s` looks at it, so token boundaries match.

use super::db::VennDb;
use crate::util::chars::{is_digit, is_hex, is_space, is_word};

/// A token kind. The order mirrors Pele's `T` enum, and the parser compares kinds by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum T {
    /// End of input, after `Eof`.
    End,
    /// Unrecognized text.
    Error,
    /// End of input.
    Eof,
    /// One or more line breaks.
    Newline,
    /// `venn-beta`.
    Venn,
    /// `title` and its text.
    Title,
    /// `set`.
    Set,
    /// `union`.
    Union,
    /// `text`.
    Text,
    /// An indented `text` after a set or union.
    IndentText,
    /// `style`.
    Style,
    /// `[label]` or `["label"]`.
    BracketLabel,
    /// A number.
    Numeric,
    /// `#rgb` and longer hex colors.
    HexColor,
    /// `rgb(r, g, b)`.
    RgbColor,
    /// `rgba(r, g, b, a)`.
    RgbaColor,
    /// A word.
    Identifier,
    /// A quoted string, quotes included.
    String,
    /// `,`.
    Comma,
    /// `:`.
    Colon,
}

impl T {
    /// The name Mermaid's parser reports, from Pele's `TOKEN_NAMES`.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::End => "$end",
            Self::Error => "INVALID",
            Self::Eof => "EOF",
            Self::Newline => "NEWLINE",
            Self::Venn => "VENN",
            Self::Title => "TITLE",
            Self::Set => "SET",
            Self::Union => "UNION",
            Self::Text => "TEXT",
            Self::IndentText => "INDENT_TEXT",
            Self::Style => "STYLE",
            Self::BracketLabel => "BRACKET_LABEL",
            Self::Numeric => "NUMERIC",
            Self::HexColor => "HEXCOLOR",
            Self::RgbColor => "RGBCOLOR",
            Self::RgbaColor => "RGBACOLOR",
            Self::Identifier => "IDENTIFIER",
            Self::String => "STRING",
            Self::Comma => "COMMA",
            Self::Colon => "COLON",
        }
    }
}

/// Keywords, in the order Pele tries them.
const KEYWORDS: [(&str, T); 5] = [
    ("venn-beta", T::Venn),
    ("set", T::Set),
    ("union", T::Union),
    ("text", T::Text),
    ("style", T::Style),
];

/// The code past the end of the source, which no test matches. Pele's `charCodeAt` returns `NaN`.
const NONE: u32 = u32::MAX;

/// Reads venn tokens one at a time.
pub(crate) struct VennLexer<'a> {
    at: usize,
    ended: bool,
    line_start: bool,
    src: &'a str,
    /// The UTF-8 offset of the token `next` returned last.
    pub(crate) start: usize,
    /// The text of the token `next` returned last.
    pub(crate) text: &'a str,
}

impl<'a> VennLexer<'a> {
    /// A lexer at the start of `src`.
    pub(crate) fn new(src: &'a str) -> Self {
        Self {
            at: 0,
            ended: false,
            line_start: false,
            src,
            start: 0,
            text: "",
        }
    }

    /// The byte at `at`, or [`NONE`] past the end. Non-ASCII bytes match no ASCII test.
    fn byte(&self, at: usize) -> u32 {
        self.src.as_bytes().get(at).map_or(NONE, |&b| u32::from(b))
    }

    /// The character at `at`, decoded whole when it is not ASCII.
    fn code(&self, at: usize) -> u32 {
        match self.src.as_bytes().get(at) {
            None => NONE,
            Some(&b) if b < 0x80 => u32::from(b),
            Some(&b) => self
                .src
                .get(at..)
                .and_then(|rest| rest.chars().next())
                .map_or(u32::from(b), u32::from),
        }
    }

    /// The length in bytes of the character at `at`.
    fn len_at(&self, at: usize) -> usize {
        self.src
            .get(at..)
            .and_then(|rest| rest.chars().next())
            .map_or(1, char::len_utf8)
    }

    fn slice(&self, from: usize, to: usize) -> &'a str {
        self.src.get(from..to).unwrap_or("")
    }

    /// Whether `word` (lowercase ASCII) starts at `at`, ignoring case.
    fn word(&self, at: usize, word: &str) -> bool {
        self.src
            .as_bytes()
            .get(at..at + word.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(word.as_bytes()))
    }

    fn keyword(&self, at: usize, word: &str) -> bool {
        self.word(at, word) && !is_word(self.byte(at + word.len()))
    }

    /// The end of `rgb(1, 2, 3)` or `rgba(1, 2, 3, 0.5)` whose parenthesis opens at `at`.
    fn color(&self, at: usize, parts: usize) -> Option<usize> {
        let mut i = at + 1;
        for part in 0..parts {
            while is_space(self.code(i)) {
                i += self.len_at(i);
            }
            let from = i;
            while is_digit(self.byte(i)) || self.byte(i) == 46 {
                i += 1;
            }
            if i == from {
                return None;
            }
            while is_space(self.code(i)) {
                i += self.len_at(i);
            }
            if self.byte(i) != if part == parts - 1 { 41 } else { 44 } {
                return None;
            }
            i += 1;
        }
        Some(i)
    }

    fn line_end(&self, from: usize) -> usize {
        self.src
            .get(from..)
            .and_then(|rest| rest.find('\n'))
            .map_or(self.src.len(), |at| from + at)
    }

    fn token(&mut self, kind: T, end: usize) -> T {
        self.text = self.slice(self.at, end);
        self.at = end;
        kind
    }

    /// Reads the next token. `host` says whether an indented `text` continues a set, and hears
    /// when a line starts with something else.
    #[expect(clippy::too_many_lines, reason = "mirrors Pele's next")]
    pub(crate) fn next(&mut self, host: &mut VennDb) -> T {
        let n = self.src.len();
        loop {
            let mut i = self.at;
            self.start = i;
            self.text = "";
            if i >= n {
                if self.ended {
                    return T::End;
                }
                self.ended = true;
                return T::Eof;
            }
            let c = self.byte(i);
            // A comment runs to the end of the line. It also takes the character before it along,
            // whatever that is: a line break, or the last letter of a word. Pele reads code units,
            // so a character outside the Basic Multilingual Plane is never the one before.
            let next = i + self.len_at(i);
            if (c == 37 && self.byte(i + 1) == 37 && self.byte(i + 2) != 123)
                || (c != 125
                    && self.len_at(i) < 4
                    && self.byte(next) == 37
                    && self.byte(next + 1) == 37)
            {
                let from = if c == 37 { i + 2 } else { next + 1 };
                self.at = self.line_end(from);
                continue;
            }
            if self.line_start {
                if c == 32 || c == 9 {
                    let mut j = i + 1;
                    while matches!(self.byte(j), 32 | 9) {
                        j += 1;
                    }
                    self.at = j;
                    if self.keyword(j, "text") && host.indent_mode() {
                        // The word `text` itself is dropped, and the indentation stands for it.
                        self.at = j + 4;
                        self.line_start = false;
                        self.text = self.slice(i, j);
                        return T::IndentText;
                    }
                    continue;
                }
                if c != 10 && c != 13 {
                    host.set_indent_mode(false);
                    self.line_start = false;
                }
            }
            if c == 10 || c == 13 {
                i += 1;
                while matches!(self.byte(i), 10 | 13) {
                    i += 1;
                }
                self.line_start = true;
                return self.token(T::Newline, i);
            }
            if c == 37 && self.byte(i + 1) == 37 {
                self.at = self.line_end(i);
                continue;
            }
            if c == 32 || c == 9 {
                i += 1;
                while matches!(self.byte(i), 32 | 9) {
                    i += 1;
                }
                self.at = i;
                continue;
            }

            if ((c | 32) >= 97 && (c | 32) <= 122) || c == 95 {
                if self.word(i, "title") && is_space(self.code(i + 5)) {
                    let body = i + 5 + self.len_at(i + 5);
                    let mut end = body;
                    while end < n && !matches!(self.byte(end), 35 | 10 | 59) {
                        end += 1;
                    }
                    if end > body {
                        return self.token(T::Title, end);
                    }
                }
                for (word, kind) in KEYWORDS {
                    if self.keyword(i, word) {
                        return self.token(kind, i + word.len());
                    }
                }
                if self.word(i, "rgb") {
                    let alpha = (self.byte(i + 3) | 32) == 97;
                    let open = i + if alpha { 4 } else { 3 };
                    let end = if self.byte(open) == 40 {
                        self.color(open, if alpha { 4 } else { 3 })
                    } else {
                        None
                    };
                    if let Some(end) = end {
                        return self.token(if alpha { T::RgbaColor } else { T::RgbColor }, end);
                    }
                }
                i += 1;
                while is_word(self.byte(i)) || self.byte(i) == 45 {
                    i += 1;
                }
                return self.token(T::Identifier, i);
            }
            if c == 91 {
                if self.byte(i + 1) == 34 {
                    let quote = self
                        .src
                        .get(i + 2..)
                        .and_then(|rest| rest.find('"'))
                        .map(|at| i + 2 + at);
                    let Some(quote) = quote.filter(|&quote| self.byte(quote + 1) == 93) else {
                        return T::Error;
                    };
                    self.at = quote + 2;
                    self.text = self.slice(i + 2, quote);
                    return T::BracketLabel;
                }
                let mut end = i + 1;
                while end < n && !matches!(self.byte(end), 93 | 34) {
                    end += 1;
                }
                if end == i + 1 || self.byte(end) != 93 {
                    return T::Error;
                }
                self.at = end + 1;
                self.text = crate::text::label::trim(self.slice(i + 1, end));
                return T::BracketLabel;
            }
            if is_digit(c) || c == 46 || c == 43 || c == 45 {
                if c == 43 || c == 45 {
                    i += 1;
                }
                let from = i;
                while is_digit(self.byte(i)) {
                    i += 1;
                }
                let fraction = self.byte(i) == 46 && is_digit(self.byte(i + 1));
                if i == from && !fraction {
                    return T::Error;
                }
                if fraction {
                    i += 2;
                    while is_digit(self.byte(i)) {
                        i += 1;
                    }
                }
                return self.token(T::Numeric, i);
            }
            if c == 35 {
                let mut end = i + 1;
                while end < i + 9 && is_hex(self.byte(end)) {
                    end += 1;
                }
                return if end >= i + 4 {
                    self.token(T::HexColor, end)
                } else {
                    T::Error
                };
            }
            if c == 34 {
                return match self.src.get(i + 1..).and_then(|rest| rest.find('"')) {
                    Some(at) => self.token(T::String, i + 1 + at + 1),
                    None => T::Error,
                };
            }
            if c == 44 {
                return self.token(T::Comma, i + 1);
            }
            if c == 58 {
                return self.token(T::Colon, i + 1);
            }
            return T::Error;
        }
    }
}
