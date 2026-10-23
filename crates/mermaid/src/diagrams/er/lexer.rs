//! The ER lexer: a state machine equivalent to Mermaid's Jison lexer. Port of Pele's
//! `src/diagrams/er/lexer.ts`.
//!
//! Pele reads UTF-16 code units. This port reads UTF-8 bytes: every delimiter and keyword is
//! ASCII, runs of non-ASCII text are runs of bytes of 128 and above, and characters are decoded
//! whole wherever `\s` or the attribute pattern tests them, so token boundaries match. Each
//! regular expression in Pele is a hand-written matcher below.

use super::tokens::T;
use crate::util::chars::{is_digit, is_line_end, is_space, is_word};

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
    AccDescr,
    AccDescrMultiline,
    AccTitle,
    Block,
    BlockBq,
    Initial,
    Style,
}

/// Characters of entity names.
const TEXT: u8 = 1;
/// Characters of style text.
const STYLE_TEXT: u8 = 2;
/// Characters that may start an attribute word.
const ATTR_START: u8 = 4;
/// Characters that may continue an attribute word.
const ATTR_REST: u8 = 8;

/// The character classes of ASCII characters.
const CLASS: [u8; 128] = {
    let mut table = [0u8; 128];
    let mut c = 0;
    while c < 128 {
        #[expect(clippy::cast_possible_truncation, reason = "c is below 128")]
        let b = c as u8;
        let word = b.is_ascii_alphanumeric() || b == b'_';
        if word {
            table[c] |= TEXT | STYLE_TEXT | ATTR_REST;
        }
        if word && !b.is_ascii_digit() {
            table[c] |= ATTR_START;
        }
        c += 1;
    }
    table[b'-' as usize] |= TEXT | STYLE_TEXT | ATTR_REST;
    table[b'*' as usize] |= TEXT | STYLE_TEXT | ATTR_REST | ATTR_START;
    table[b'.' as usize] |= TEXT | ATTR_REST;
    table[b'[' as usize] |= ATTR_REST;
    table[b']' as usize] |= ATTR_REST;
    table[b'(' as usize] |= ATTR_REST;
    table[b')' as usize] |= ATTR_REST;
    table[b',' as usize] |= ATTR_REST;
    table
};

/// Whether byte `c` is in class `kind`, any byte of a non-ASCII character counting as text.
fn is_text(c: Option<u8>, kind: u8) -> bool {
    c.is_some_and(|c| c >= 128 || CLASS.get(usize::from(c)).is_some_and(|&k| k & kind != 0))
}

/// Mermaid's attribute pattern is case-insensitive, which lets the micro sign into its
/// `À-￿` range.
fn is_attr(c: char, kind: u8) -> bool {
    let code = u32::from(c);
    if code < 128 {
        CLASS
            .get(usize::try_from(code).unwrap_or(0))
            .is_some_and(|&k| k & kind != 0)
    } else {
        code >= 0xc0 || code == 0xb5
    }
}

/// The character at byte `at`.
fn char_at(src: &str, at: usize) -> Option<char> {
    src.get(at..).and_then(|rest| rest.chars().next())
}

/// Whether the character at `at` is JavaScript `\s`.
fn space_at(src: &str, at: usize) -> bool {
    char_at(src, at).is_some_and(|c| is_space(u32::from(c)))
}

/// The end of a run of JavaScript `\s` characters from `at`.
fn skip_spaces(src: &str, at: usize) -> usize {
    let rest = src.get(at..).unwrap_or("");
    at + rest
        .char_indices()
        .find(|&(_, c)| !is_space(u32::from(c)))
        .map_or(rest.len(), |(i, _)| i)
}

/// The first `byte` at or after `from`.
fn find(src: &str, from: usize, byte: u8) -> Option<usize> {
    src.as_bytes()
        .get(from..)
        .and_then(|rest| rest.iter().position(|&b| b == byte))
        .map(|at| from + at)
}

/// Whether `src` at `at` spells the ASCII `word`, letters in any case.
fn starts_ci(src: &str, at: usize, word: &str) -> bool {
    src.as_bytes()
        .get(at..at + word.len())
        .is_some_and(|text| text.eq_ignore_ascii_case(word.as_bytes()))
}

/// The first `direction`, in any case, at or after `from`.
fn find_direction(src: &str, from: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    (from..bytes.len()).find(|&at| starts_ci(src, at, "direction"))
}

/// The end of the next line terminator's line from `from`: its index, or the end of `src`.
fn next_line_end(src: &str, from: usize) -> usize {
    src.get(from..)
        .and_then(|rest| rest.find(|c: char| is_line_end(u32::from(c))))
        .map_or(src.len(), |at| from + at)
}

/// `/<word>\s*<sep>\s*/iy` at `at`: the end of the match.
fn acc(src: &str, at: usize, word: &str, sep: u8) -> Option<usize> {
    if !starts_ci(src, at, word) {
        return None;
    }
    let q = skip_spaces(src, at + word.len());
    if src.as_bytes().get(q) != Some(&sep) {
        return None;
    }
    Some(skip_spaces(src, q + 1))
}

/// Where `/.*direction\s+XX[^\n]*/iy` ends when it matches at `p`. `.*` is greedy, so the last
/// `direction` on the line that is followed by the pair wins.
fn direction_end(src: &str, p: usize, line_end: usize, pair: &str) -> Option<usize> {
    let mut candidates = Vec::new();
    let mut at = p;
    while at < line_end {
        if starts_ci(src, at, "direction") && at + 9 <= line_end {
            candidates.push(at);
        }
        at += 1;
    }
    candidates.into_iter().rev().find_map(|start| {
        let q = skip_spaces(src, start + 9);
        (q > start + 9 && starts_ci(src, q, pair))
            .then(|| find(src, q + 2, b'\n').unwrap_or(src.len()))
    })
}

/// Where the next occurrence of something the lexer looks ahead for is.
#[derive(Clone, Copy)]
enum Ahead {
    /// There is none further on.
    Gone,
    /// It was last found here.
    At(usize),
    /// Not searched for yet.
    Unknown,
}

/// Produces the token stream of Mermaid's ER lexer.
pub(crate) fn tokenize(src: &str) -> Vec<Token<'_>> {
    Lexer {
        dir_failed: 0,
        dir_next: Ahead::Unknown,
        line_end: None,
        p: 0,
        src,
        state: S::Initial,
        tilde_last: None,
        tilde_line: None,
        tilde_next: Ahead::Unknown,
        tokens: Vec::new(),
        ws_next: None,
    }
    .run()
}

/// The lexer's position, state, and the positions it looks ahead to.
struct Lexer<'a> {
    dir_failed: usize,
    /// The next `direction`.
    dir_next: Ahead,
    line_end: Option<usize>,
    p: usize,
    src: &'a str,
    state: S,
    tilde_last: Option<usize>,
    tilde_line: Option<usize>,
    /// The next `~`.
    tilde_next: Ahead,
    tokens: Vec<Token<'a>>,
    ws_next: Option<usize>,
}

impl<'a> Lexer<'a> {
    /// The byte at `i`.
    fn at(&self, i: usize) -> Option<u8> {
        self.src.as_bytes().get(i).copied()
    }

    /// Emits a token from `s` to `e` and moves to `e`.
    fn emit(&mut self, kind: T, s: usize, e: usize) {
        self.tokens.push(Token {
            kind,
            start: s,
            text: self.src.get(s..e).unwrap_or(""),
        });
        self.p = e;
    }

    /// Matches a lower-case keyword at the position, ignoring case.
    fn kw(&self, word: &str) -> bool {
        starts_ci(self.src, self.p, word)
    }

    /// Matches a lower-case keyword at the position, ignoring case, at a word boundary.
    fn kwb(&self, word: &str) -> bool {
        self.kw(word)
            && !self
                .at(self.p + word.len())
                .is_some_and(|c| is_word(u32::from(c)))
    }

    /// The byte at `q`, lower-cased if it is an ASCII letter.
    fn lower(&self, q: usize) -> Option<u8> {
        self.at(q).map(|c| c.to_ascii_lowercase())
    }

    /// The end of a run of digits from `q`.
    fn digits(&self, mut q: usize) -> usize {
        while self.at(q).is_some_and(|c| c.is_ascii_digit()) {
            q += 1;
        }
        q
    }

    /// A direction statement swallows its whole line, from wherever the lexer stands on it.
    fn direction(&mut self) -> bool {
        let p = self.p;
        let next = match self.dir_next {
            Ahead::Gone => return false,
            Ahead::At(next) if next >= p => next,
            _ => {
                let Some(next) = find_direction(self.src, p) else {
                    self.dir_next = Ahead::Gone;
                    return false;
                };
                self.dir_next = Ahead::At(next);
                next
            }
        };
        if p < self.dir_failed {
            return false;
        }
        let line_end = match self.line_end {
            Some(end) if end >= p => end,
            _ => {
                let end = next_line_end(self.src, p);
                self.line_end = Some(end);
                end
            }
        };
        if next >= line_end {
            return false;
        }
        let kinds = [
            (T::DirectionTb, "TB"),
            (T::DirectionBt, "BT"),
            (T::DirectionRl, "RL"),
            (T::DirectionLr, "LR"),
        ];
        for (kind, pair) in kinds {
            if let Some(end) = direction_end(self.src, p, line_end, pair) {
                self.emit(kind, p, end);
                return true;
            }
        }
        self.dir_failed = line_end;
        false
    }

    /// A double-quoted entity name, or a quoted word that cannot be one.
    fn quoted(&mut self) {
        let p = self.p;
        let Some(e) = find(self.src, p + 1, b'"') else {
            self.emit(T::Char, p, p + 1);
            return;
        };
        let name = e > p + 1
            && !self.src.as_bytes().get(p + 1..e).is_some_and(|text| {
                text.iter()
                    .any(|c| matches!(c, b'%' | b'\\' | b'\r' | b'\n' | 11 | 8))
            });
        self.emit(if name { T::EntityName } else { T::Word }, p, e + 1);
    }

    /// A number, or a cardinality written with digits.
    #[expect(clippy::many_single_char_names, reason = "keeps Pele's names")]
    fn number(&mut self, c: u8) {
        let p = self.p;
        let d = self.at(p + 1);
        if d == Some(b'+') && (c == b'1' || c == b'0') {
            let kind = if c == b'1' {
                T::OneOrMore
            } else {
                T::ZeroOrMore
            };
            self.emit(kind, p, p + 2);
            return;
        }
        let q = self.digits(p + 1);
        if self.at(q) == Some(b'.') && self.at(q + 1).is_some_and(|c| c.is_ascii_digit()) {
            let e = self.digits(q + 2);
            self.emit(T::DecimalNum, p, e);
            return;
        }
        if c == b'1' {
            if let Some(space) = char_at(self.src, p + 1).filter(|&d| is_space(u32::from(d))) {
                let r = skip_spaces(self.src, p + 1 + space.len_utf8());
                if self
                    .at(r)
                    .is_some_and(|f| is_word(u32::from(f)) || f == b'"' || f == b'\'')
                {
                    self.emit(T::OnlyOne, p, p + 1);
                    return;
                }
            } else if matches!(d, Some(b'-' | b'.')) && matches!(self.at(p + 2), Some(b'-' | b'.'))
            {
                self.emit(T::OnlyOne, p, p + 1);
                return;
            }
            if !d.is_some_and(|d| is_word(u32::from(d))) {
                self.emit(T::EntityOne, p, p + 1);
                return;
            }
        }
        self.emit(T::Num, p, q);
    }

    /// The keywords and relationship symbols, by lower-cased first character.
    #[expect(
        clippy::too_many_lines,
        reason = "one arm per keyword letter, as in Pele"
    )]
    fn keyword(&mut self, lc: u8) -> bool {
        let p = self.p;
        let d = self.at(p + 1);
        match lc {
            b'e' => {
                if self.kwb("erdiagram") {
                    self.emit(T::ErDiagram, p, p + 9);
                } else if self.kwb("end") {
                    let e = skip_spaces(self.src, p + 3);
                    self.emit(T::EndKw, p, e);
                } else {
                    return false;
                }
            }
            b's' => {
                if self.kwb("style") {
                    self.state = S::Style;
                    self.emit(T::Style, p, p + 5);
                } else if self.kwb("subgraph") {
                    self.emit(T::Subgraph, p, p + 8);
                } else {
                    return false;
                }
            }
            b'c' => {
                if self.kwb("classdef") {
                    self.state = S::Style;
                    self.emit(T::Classdef, p, p + 8);
                } else if self.kwb("class") {
                    self.emit(T::Class, p, p + 5);
                } else {
                    return false;
                }
            }
            b'o' => {
                if self.kwb("one or zero") {
                    self.emit(T::ZeroOrOne, p, p + 11);
                } else if self.kwb("one or more") || self.kwb("one or many") {
                    self.emit(T::OneOrMore, p, p + 11);
                } else if self.kwb("one") {
                    self.emit(T::OnlyOne, p, p + 3);
                } else if self.kwb("only one") {
                    self.emit(T::OnlyOne, p, p + 8);
                } else if d == Some(b'|') {
                    self.emit(T::ZeroOrOne, p, p + 2);
                } else if d == Some(b'{') {
                    self.emit(T::ZeroOrMore, p, p + 2);
                } else if self.kwb("optionally to") {
                    self.emit(T::NonIdentifying, p, p + 13);
                } else {
                    return false;
                }
            }
            b'z' => {
                if self.kwb("zero or one") {
                    self.emit(T::ZeroOrOne, p, p + 11);
                } else if self.kwb("zero or more") || self.kwb("zero or many") {
                    self.emit(T::ZeroOrMore, p, p + 12);
                } else {
                    return false;
                }
            }
            b'm' => {
                if self.kw("many(0)") {
                    self.emit(T::ZeroOrMore, p, p + 7);
                } else if self.kw("many(1)") {
                    self.emit(T::OneOrMore, p, p + 7);
                } else if self.kwb("many") {
                    self.emit(T::ZeroOrMore, p, p + 4);
                } else {
                    return false;
                }
            }
            b't' => {
                if !self.kwb("to") {
                    return false;
                }
                self.emit(T::Identifying, p, p + 2);
            }
            b'u' => {
                if !matches!(d, Some(b'.' | b'-' | b'|')) {
                    return false;
                }
                self.emit(T::MdParent, p, p + 1);
            }
            b'|' => {
                let o = self.lower(p + 1) == Some(b'o')
                    && !self.at(p + 2).is_some_and(|c| is_word(u32::from(c)));
                if o {
                    self.emit(T::ZeroOrOne, p, p + 2);
                } else if d == Some(b'|') {
                    self.emit(T::OnlyOne, p, p + 2);
                } else if d == Some(b'{') {
                    self.emit(T::OneOrMore, p, p + 2);
                } else {
                    return false;
                }
            }
            b'}' => {
                let o = self.lower(p + 1) == Some(b'o')
                    && !self.at(p + 2).is_some_and(|c| is_word(u32::from(c)));
                if o {
                    self.emit(T::ZeroOrMore, p, p + 2);
                } else if d == Some(b'|') {
                    self.emit(T::OneOrMore, p, p + 2);
                } else {
                    return false;
                }
            }
            b'.' => {
                if !matches!(d, Some(b'.' | b'-')) {
                    return false;
                }
                self.emit(T::NonIdentifying, p, p + 2);
            }
            b'-' => {
                if d == Some(b'-') {
                    self.emit(T::Identifying, p, p + 2);
                } else if d == Some(b'.') {
                    self.emit(T::NonIdentifying, p, p + 2);
                } else {
                    return false;
                }
            }
            _ => return false,
        }
        true
    }

    /// The rules of the initial state.
    fn initial(&mut self, c: u8) {
        let p = self.p;
        if (c == b'a' || c == b'A') && self.kw("acc") {
            if let Some(e) = acc(self.src, p, "accTitle", b':') {
                self.state = S::AccTitle;
                self.emit(T::AccTitle, p, e);
                return;
            }
            if let Some(e) = acc(self.src, p, "accDescr", b':') {
                self.state = S::AccDescr;
                self.emit(T::AccDescr, p, e);
                return;
            }
            if let Some(e) = acc(self.src, p, "accDescr", b'{') {
                self.state = S::AccDescrMultiline;
                self.p = e;
                return;
            }
        }
        if self.direction() {
            return;
        }

        match c {
            b' ' | b'\t' | b'\r' => {
                let mut e = p + 1;
                while matches!(self.at(e), Some(b' ' | b'\t' | b'\r')) {
                    e += 1;
                }
                self.p = e;
                return;
            }
            b'\n' => {
                let mut e = p + 1;
                while self.at(e) == Some(b'\n') {
                    e += 1;
                }
                self.emit(T::Newline, p, e);
                return;
            }
            b'"' => {
                self.quoted();
                return;
            }
            b'{' => {
                self.state = S::Block;
                self.emit(T::BlockStart, p, p + 1);
                return;
            }
            b'#' => {
                self.emit(T::Brkt, p, p + 1);
                return;
            }
            b',' => {
                self.emit(T::Comma, p, p + 1);
                return;
            }
            b':' => {
                if self.at(p + 1) == Some(b':') && self.at(p + 2) == Some(b':') {
                    self.emit(T::StyleSeparator, p, p + 3);
                } else {
                    self.emit(T::Colon, p, p + 1);
                }
                return;
            }
            b'[' => {
                self.emit(T::Sqs, p, p + 1);
                return;
            }
            b']' => {
                self.emit(T::Sqe, p, p + 1);
                return;
            }
            _ => {}
        }
        if is_digit(u32::from(c)) {
            self.number(c);
            return;
        }
        if self.keyword(c.to_ascii_lowercase()) {
            return;
        }
        if is_text(Some(c), TEXT) {
            let mut e = p + 1;
            while is_text(self.at(e), TEXT) {
                e += 1;
            }
            self.emit(T::UnicodeText, p, e);
            return;
        }
        self.emit(T::Char, p, p + 1);
    }

    /// Mermaid's generic type rule: a run of non-space characters holding a `~`, through the
    /// last `~` on the line.
    fn generic(&mut self) -> bool {
        let p = self.p;
        let tilde_next = match self.tilde_next {
            Ahead::Gone => return false,
            Ahead::At(next) if next >= p => next,
            _ => {
                let Some(next) = find(self.src, p, b'~') else {
                    self.tilde_next = Ahead::Gone;
                    return false;
                };
                self.tilde_next = Ahead::At(next);
                next
            }
        };
        let ws_next = match self.ws_next {
            Some(ws) if ws >= p => ws,
            _ => {
                let mut q = p;
                while let Some(c) = char_at(self.src, q) {
                    if is_space(u32::from(c)) {
                        break;
                    }
                    q += c.len_utf8();
                }
                self.ws_next = Some(q);
                q
            }
        };
        if tilde_next >= ws_next {
            return false;
        }
        if self.tilde_line.is_none_or(|line| line < p) {
            let line = next_line_end(self.src, p);
            self.tilde_line = Some(line);
            self.tilde_last = self
                .src
                .as_bytes()
                .get(..line)
                .and_then(|head| head.iter().rposition(|&b| b == b'~'));
        }
        let Some(last) = self.tilde_last.filter(|&last| last > tilde_next) else {
            return false;
        };
        let mut e = last + 1;
        while let Some(c) = char_at(self.src, e) {
            if is_space(u32::from(c)) {
                break;
            }
            e += c.len_utf8();
        }
        self.emit(T::AttributeWord, p, e);
        true
    }

    /// The rules inside an attribute block.
    fn block(&mut self, c: u8) {
        let p = self.p;
        if space_at(self.src, p) {
            self.p = skip_spaces(self.src, p);
            return;
        }
        let lc = c | 32;
        if matches!(lc, b'p' | b'f' | b'u')
            && self.at(p + 1).is_some_and(|d| d | 32 == b'k')
            && !self.at(p + 2).is_some_and(|d| is_word(u32::from(d)))
        {
            self.emit(T::AttributeKey, p, p + 2);
            return;
        }
        if self.generic() {
            return;
        }
        let ch = char_at(self.src, p).unwrap_or('\0');
        if is_attr(ch, ATTR_START) {
            let mut e = p + ch.len_utf8();
            while let Some(d) = char_at(self.src, e).filter(|&d| is_attr(d, ATTR_REST)) {
                e += d.len_utf8();
            }
            self.emit(T::AttributeWord, p, e);
            return;
        }
        if c == b'`' {
            self.state = S::BlockBq;
            self.p += 1;
            return;
        }
        if c == b'"' {
            if let Some(e) = find(self.src, p + 1, b'"') {
                self.emit(T::Comment, p, e + 1);
                return;
            }
        } else if c == b'}' {
            self.state = S::Initial;
            self.emit(T::BlockStop, p, p + 1);
            return;
        }
        self.emit(T::Char, p, p + ch.len_utf8());
    }

    /// Reads the whole source.
    fn run(mut self) -> Vec<Token<'a>> {
        let n = self.src.len();
        while let Some(c) = self.at(self.p) {
            let p = self.p;
            match self.state {
                S::Initial => self.initial(c),
                S::Block => self.block(c),
                S::BlockBq => {
                    if c == b'`' {
                        self.state = S::Block;
                        self.p += 1;
                    } else {
                        let e = find(self.src, p, b'`').unwrap_or(n);
                        self.emit(T::AttributeWord, p, e);
                    }
                }
                S::AccTitle | S::AccDescr => {
                    let e = find(self.src, p, b'\n').unwrap_or(n);
                    let kind = if self.state == S::AccTitle {
                        T::AccTitleValue
                    } else {
                        T::AccDescrValue
                    };
                    self.emit(kind, p, e);
                    self.state = S::Initial;
                }
                S::AccDescrMultiline => {
                    if c == b'}' {
                        self.state = S::Initial;
                        self.p += 1;
                    } else {
                        let e = find(self.src, p, b'}').unwrap_or(n);
                        self.emit(T::AccDescrMultilineValue, p, e);
                    }
                }
                S::Style => {
                    if c == b'\n' {
                        let mut e = p + 1;
                        while self.at(e) == Some(b'\n') {
                            e += 1;
                        }
                        self.state = S::Initial;
                        self.emit(T::Newline, p, e);
                    } else if space_at(self.src, p) {
                        self.p = skip_spaces(self.src, p);
                    } else if c == b':' {
                        self.emit(T::Colon, p, p + 1);
                    } else if c == b',' {
                        self.emit(T::Comma, p, p + 1);
                    } else if c == b'#' {
                        self.emit(T::Brkt, p, p + 1);
                    } else if c == b';' {
                        self.emit(T::Semi, p, p + 1);
                    } else if is_text(Some(c), STYLE_TEXT) {
                        let mut e = p + 1;
                        while is_text(self.at(e), STYLE_TEXT) {
                            e += 1;
                        }
                        self.emit(T::StyleText, p, e);
                    } else {
                        self.tokens.push(Token {
                            kind: T::Error,
                            start: p,
                            text: "",
                        });
                        return self.tokens;
                    }
                }
            }
        }

        // At the end of input Jison's INITIAL state yields EOF, the title and description states
        // yield an empty value, and the block and style states yield nothing.
        let last = match self.state {
            S::Initial => Some(T::Eof),
            S::AccTitle => Some(T::AccTitleValue),
            S::AccDescr => Some(T::AccDescrValue),
            S::AccDescrMultiline => Some(T::AccDescrMultilineValue),
            S::Block | S::BlockBq | S::Style => None,
        };
        if let Some(last) = last {
            self.emit(last, n, n);
        }
        self.emit(T::End, n, n);
        self.tokens
    }
}
