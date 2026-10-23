//! The quadrant chart lexer. Port of Pele's `src/diagrams/quadrant/lexer.ts`.
//!
//! Pele reads UTF-16 code units. This port reads UTF-8 bytes. Every delimiter is ASCII, a
//! non-ASCII character is decoded whole wherever a rule tests it, and a character outside the
//! Basic Multilingual Plane counts as the two units a rule would see in JavaScript.

use super::tokens::T;
use crate::diagrams::common::caseless::word_at;
use crate::util::chars::{is_digit, is_letter, is_line_end, is_space, is_word};

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

/// A start condition of Mermaid's lexer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum S {
    Initial,
    Title,
    AccTitle,
    AccDescr,
    AccDescrMultiline,
    String,
    MdString,
    ClassName,
    PointStart,
    PointX,
    PointY,
}

impl S {
    /// The token the state gives for the empty rest of the input, where it gives one.
    const fn at_end(self) -> Option<T> {
        match self {
            Self::Initial => Some(T::Eof),
            Self::Title => Some(T::TitleValue),
            Self::AccTitle => Some(T::AccTitleValue),
            Self::AccDescr => Some(T::AccDescrValue),
            Self::AccDescrMultiline => Some(T::AccDescrMultilineValue),
            Self::String => Some(T::Str),
            _ => None,
        }
    }
}

/// The token of a one-character ASCII rule.
fn single(c: u8) -> Option<T> {
    Some(match c {
        b':' => T::Colon,
        b'+' => T::Plus,
        b',' => T::Comma,
        b'=' => T::Equals,
        b'*' => T::Mult,
        b'#' => T::Brkt,
        b'_' => T::Underscore,
        b'.' => T::Dot,
        b'&' => T::Amp,
        b'-' => T::Minus,
        b' ' | b'\t' | 0x0b | 0x0c => T::Space,
        b';' => T::Semi,
        b'!' | b'$' | b'%' | b'\'' | b'`' | b'?' | b'\\' | b'/' => T::Punctuation,
        _ => return None,
    })
}

/// The character at `at`, or `None` at the end.
fn char_at(src: &str, at: usize) -> Option<char> {
    src.get(at..).and_then(|rest| rest.chars().next())
}

/// The byte at `at`, or 0 at the end.
fn byte(src: &str, at: usize) -> u8 {
    src.as_bytes().get(at).copied().unwrap_or(0)
}

/// Lexer state over one source.
struct Lexer<'a> {
    dash: (usize, usize),
    gap: (usize, usize),
    p: usize,
    space: (usize, usize),
    src: &'a str,
    stack: Vec<S>,
    state: S,
    tokens: Vec<Token>,
    ws: (usize, usize),
}

impl Lexer<'_> {
    fn emit(&mut self, t: T, start: usize, end: usize) {
        self.tokens.push(Token { end, start, t });
        self.p = end;
    }

    fn push(&mut self, state: S) {
        self.stack.push(self.state);
        self.state = state;
    }

    fn pop(&mut self) {
        if let Some(state) = self.stack.pop() {
            self.state = state;
        }
    }

    fn line_end(&self, q: usize) -> usize {
        self.src
            .get(q..)
            .and_then(|rest| rest.find('\n'))
            .map_or(self.src.len(), |at| q + at)
    }

    /// The end of the run of spaces at `q`.
    fn spaces(&self, mut q: usize) -> usize {
        while byte(self.src, q) == b' ' {
            q += 1;
        }
        q
    }

    /// The end of the run of `\s` at `q`.
    fn run(&self, mut q: usize) -> usize {
        while let Some(c) = char_at(self.src, q).filter(|&c| is_space(u32::from(c))) {
            q += c.len_utf8();
        }
        q
    }

    /// Mermaid's `(1)|(0(.\d+)?)`, where the dot is any one code unit within a line.
    fn coordinate(&self, q: usize) -> Option<usize> {
        match byte(self.src, q) {
            b'1' => Some(q + 1),
            b'0' => {
                let mut e = q + 1;
                if let Some(any) = char_at(self.src, e)
                    && !is_line_end(u32::from(any))
                    && any.len_utf16() == 1
                    && is_digit(u32::from(byte(self.src, e + any.len_utf8())))
                {
                    e += any.len_utf8() + 1;
                    while is_digit(u32::from(byte(self.src, e))) {
                        e += 1;
                    }
                }
                Some(e)
            }
            _ => None,
        }
    }

    /// Runs the rule of a state that is not the initial one. Returns `None` to go on with the
    /// initial rules, `Some(true)` when a rule matched, and `Some(false)` when nothing can.
    fn state_rule(&mut self, c: u8) -> Option<bool> {
        let p = self.p;
        let src = self.src;
        let n = src.len();
        match self.state {
            S::Initial => None,
            S::Title | S::AccTitle | S::AccDescr => {
                let end = self.line_end(p);
                if let Some(t) = self.state.at_end() {
                    self.emit(t, p, end);
                }
                self.pop();
                Some(true)
            }
            S::AccDescrMultiline => {
                if c == b'}' {
                    self.p += 1;
                    self.pop();
                } else {
                    let end = src
                        .get(p..)
                        .and_then(|rest| rest.find('}'))
                        .map_or(n, |at| p + at);
                    self.emit(T::AccDescrMultilineValue, p, end);
                }
                Some(true)
            }
            S::String => {
                if c == b'"' {
                    self.p += 1;
                    self.pop();
                } else {
                    let end = src
                        .get(p..)
                        .and_then(|rest| rest.find('"'))
                        .map_or(n, |at| p + at);
                    self.emit(T::Str, p, end);
                }
                Some(true)
            }
            S::MdString => {
                if c == b'`' && byte(src, p + 1) == b'"' {
                    self.p += 2;
                    self.pop();
                    return Some(true);
                }
                if c != b'`' && c != b'"' {
                    let mut e = p + char_at(src, p).map_or(1, char::len_utf8);
                    while e < n && !matches!(byte(src, e), b'`' | b'"') {
                        e += 1;
                    }
                    self.emit(T::MdStr, p, e);
                    return Some(true);
                }
                Some(false)
            }
            S::ClassName => {
                if is_word(u32::from(c)) {
                    let mut e = p + 1;
                    while is_word(u32::from(byte(src, e))) {
                        e += 1;
                    }
                    self.emit(T::ClassName, p, e);
                    self.pop();
                    return Some(true);
                }
                Some(false)
            }
            S::PointStart => {
                if let Some(e) = self.coordinate(p) {
                    self.emit(T::PointX, p, e);
                    self.push(S::PointX);
                    return Some(true);
                }
                let k = self.run(p);
                if byte(src, k) == b']' {
                    self.p = self.spaces(k + 1);
                    self.pop();
                    return Some(true);
                }
                Some(false)
            }
            S::PointX => {
                let k = self.run(p);
                if byte(src, k) == b',' {
                    self.p = self.run(k + 1);
                    self.state = S::PointY;
                    return Some(true);
                }
                Some(false)
            }
            S::PointY => {
                if let Some(e) = self.coordinate(p) {
                    self.emit(T::PointY, p, e);
                    self.pop();
                    return Some(true);
                }
                Some(false)
            }
        }
    }

    /// Runs the initial rules at `p`. Returns false when none matches.
    #[expect(
        clippy::many_single_char_names,
        reason = "mirrors the names of Pele's lexer"
    )]
    fn initial_rule(&mut self) -> bool {
        let p = self.p;
        let src = self.src;
        let Some(ch) = char_at(src, p) else {
            return false;
        };
        let c = u32::from(ch);
        let width = ch.len_utf8();
        // The code unit after the first one at `p`: a low surrogate for a character outside the
        // Basic Multilingual Plane.
        let next = if ch.len_utf16() == 2 {
            0xdc00
        } else {
            u32::from(byte(src, p + width))
        };

        if c == 37 && next == 37 {
            self.p = self.line_end(p + 2);
            return true;
        }
        // Mermaid's second comment rule takes any character before `%%` with it, a line break included.
        if c != 125 && next == 37 && byte(src, p + width + 1) == b'%' {
            self.p = self.line_end(p + width + 2);
            return true;
        }
        if c == 10 || c == 13 {
            let mut e = p + 1;
            while matches!(byte(src, e), b'\n' | b'\r') {
                e += 1;
            }
            self.emit(T::Newline, p, e);
            return true;
        }

        let lower = c | 32;
        if lower == 116 && word_at(src, p, "title") && !is_word(u32::from(byte(src, p + 5))) {
            self.emit(T::Title, p, p + 5);
            self.push(S::Title);
            return true;
        }
        if lower == 97 && word_at(src, p, "acc") {
            let descr = word_at(src, p + 3, "descr");
            if descr || word_at(src, p + 3, "title") {
                let k = self.run(p + 8);
                let d = byte(src, k);
                if d == b':' {
                    let end = self.run(k + 1);
                    self.emit(if descr { T::AccDescr } else { T::AccTitle }, p, end);
                    self.push(if descr { S::AccDescr } else { S::AccTitle });
                    return true;
                }
                if d == b'{' && descr {
                    self.p = self.run(k + 1);
                    self.push(S::AccDescrMultiline);
                    return true;
                }
            }
        }

        // Axis, quadrant and chart keywords and the axis arrow take the spaces around them.
        let mut j = p;
        if c == 32 {
            if p < self.space.0 || p >= self.space.1 {
                self.space = (p, self.spaces(p));
            }
            j = self.space.1;
        }
        let d = byte(src, j);
        let dl = d | 32;
        let mut found = None;
        if dl == b'x' && word_at(src, j, "x-axis") {
            found = Some((T::XAxis, j + 6));
        } else if dl == b'y' && word_at(src, j, "y-axis") {
            found = Some((T::YAxis, j + 6));
        } else if d == b'-' {
            if j < self.dash.0 || j >= self.dash.1 {
                let mut to = j;
                while byte(src, to) == b'-' {
                    to += 1;
                }
                self.dash = (j, to);
            }
            if self.dash.1 - j >= 2 && byte(src, self.dash.1) == b'>' {
                found = Some((T::Delimiter, self.dash.1 + 1));
            }
        } else if dl == b'q' && word_at(src, j, "quadrant") {
            let q = byte(src, j + 9);
            if byte(src, j + 8) == b'-' && (b'1'..=b'4').contains(&q) {
                let t = match q {
                    b'1' => T::Quadrant1,
                    b'2' => T::Quadrant2,
                    b'3' => T::Quadrant3,
                    _ => T::Quadrant4,
                };
                found = Some((t, j + 10));
            } else if word_at(src, j + 8, "chart") {
                found = Some((T::Quadrant, j + 13));
            }
        }
        if let Some((t, end)) = found {
            let end = self.spaces(end);
            self.emit(t, p, end);
            return true;
        }

        if lower == 99 && word_at(src, p, "classdef") && !is_word(u32::from(byte(src, p + 8))) {
            self.emit(T::Classdef, p, p + 8);
            return true;
        }
        if c == 34 {
            if next == 96 {
                self.p += 2;
                self.push(S::MdString);
            } else {
                self.p += 1;
                self.push(S::String);
            }
            return true;
        }
        if c == 58 && next == 58 && byte(src, p + 2) == b':' {
            self.p += 3;
            self.push(S::ClassName);
            return true;
        }
        // A point's `: [`, with any whitespace before, between and after, line breaks included.
        if c == 58 || is_space(c) {
            let mut k = p;
            if c != 58 {
                if p < self.ws.0 || p >= self.ws.1 {
                    self.ws = (p, self.run(p));
                }
                k = self.ws.1;
            }
            if byte(src, k) == b':' {
                if k + 1 < self.gap.0 || k + 1 >= self.gap.1 {
                    self.gap = (k + 1, self.run(k + 1));
                }
                if byte(src, self.gap.1) == b'[' {
                    let end = self.run(self.gap.1 + 1);
                    self.emit(T::PointStart, p, end);
                    self.push(S::PointStart);
                    return true;
                }
            }
        }

        if is_letter(c) {
            let mut e = p + 1;
            while is_letter(u32::from(byte(src, e))) {
                e += 1;
            }
            self.emit(T::Alpha, p, e);
            return true;
        }
        if c >= 128 {
            let mut e = p;
            while let Some(other) = char_at(src, e).filter(|&other| u32::from(other) >= 128) {
                e += other.len_utf8();
            }
            self.emit(T::UnicodeText, p, e);
            return true;
        }
        if is_digit(c) {
            let mut e = p + 1;
            while is_digit(u32::from(byte(src, e))) {
                e += 1;
            }
            self.emit(T::Num, p, e);
            return true;
        }
        if let Some(t) = u8::try_from(c).ok().and_then(single) {
            self.emit(t, p, p + 1);
            return true;
        }
        false
    }
}

/// Produces the tokens of Mermaid's Jison lexer for quadrant charts. An unrecognized character
/// ends the list with an `Error` token. Otherwise the list ends with what the last state gives
/// for the empty rest of the input, then `End`.
pub(crate) fn tokenize(src: &str) -> Vec<Token> {
    let n = src.len();
    let mut lexer = Lexer {
        dash: (0, 0),
        gap: (0, 0),
        p: 0,
        space: (0, 0),
        src,
        stack: Vec::new(),
        state: S::Initial,
        tokens: Vec::new(),
        ws: (0, 0),
    };

    while lexer.p < n {
        let matched = match lexer.state_rule(byte(src, lexer.p)) {
            Some(matched) => matched,
            None => lexer.initial_rule(),
        };
        if !matched {
            break;
        }
    }

    let mut tokens = lexer.tokens;
    if lexer.p < n {
        tokens.push(Token {
            end: lexer.p,
            start: lexer.p,
            t: T::Error,
        });
        return tokens;
    }
    // Jison tries the rules of the current state once more against the empty rest of the input.
    if let Some(t) = lexer.state.at_end() {
        tokens.push(Token {
            end: n,
            start: n,
            t,
        });
    }
    tokens.push(Token {
        end: n,
        start: n,
        t: T::End,
    });
    tokens
}
