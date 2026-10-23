//! The class lexer: a state machine equivalent to Mermaid's Jison lexer. Port of Pele's
//! `src/diagrams/class/lexer.ts`.
//!
//! Pele reads UTF-16 code units. This port reads UTF-8 bytes: every delimiter and keyword is
//! ASCII, and non-ASCII characters are decoded whole wherever `\s` or a letter test reads them,
//! so token boundaries match. Each regular expression in Pele is a hand-written matcher below.

use super::tokens::T;
use crate::util::chars::{is_line_end, is_space, is_word};
use crate::util::unicode::is_unicode_letter;

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
    Bqstring,
    CallbackArgs,
    CallbackName,
    Cls,
    ClassBody,
    Generic,
    Initial,
    Namespace,
    NamespaceBody,
    String,
}

/// The token each state gives at the end of input, before the parser sees the end itself.
fn at_end(state: S) -> T {
    match state {
        S::Initial | S::Bqstring | S::Cls | S::Namespace => T::Eof,
        S::String => T::Str,
        S::Generic => T::Generictype,
        S::CallbackName => T::CallbackName,
        S::CallbackArgs => T::CallbackArgs,
        S::AccTitle => T::AccTitleValue,
        S::AccDescr => T::AccDescrValue,
        S::AccDescrMultiline => T::AccDescrMultilineValue,
        S::ClassBody | S::NamespaceBody => T::EofInStruct,
    }
}

/// The character at byte `at`, as a code point, or `None` past the end.
fn char_at(src: &str, at: usize) -> Option<u32> {
    src.get(at..)
        .and_then(|rest| rest.chars().next())
        .map(u32::from)
}

/// Whether the character at `at` is JavaScript `\s`.
fn space_at(src: &str, at: usize) -> bool {
    char_at(src, at).is_some_and(is_space)
}

/// The end of a run of JavaScript `\s` characters from `at`.
fn skip_spaces(src: &str, at: usize) -> usize {
    let rest = src.get(at..).unwrap_or("");
    at + rest
        .char_indices()
        .find(|&(_, c)| !is_space(u32::from(c)))
        .map_or(rest.len(), |(i, _)| i)
}

/// The first `byte` at or after `from`, or the end of `src`.
fn until(src: &str, from: usize, byte: u8) -> usize {
    src.as_bytes()
        .get(from..)
        .and_then(|rest| rest.iter().position(|&b| b == byte))
        .map_or(src.len(), |at| from + at)
}

/// Where a direction statement that the lexer could take at `p` ends, as
/// `/.*direction\s+XX[^\n]*/y` matches: `kind` is 0 to 3 for `TB`, `BT`, `RL`, and `LR`.
/// `.*` is greedy, so the last `direction` on the line that is followed by the pair wins.
fn direction_end(src: &str, p: usize, line_end: usize, pair: &str) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut candidates = Vec::new();
    let mut at = p;
    while let Some(found) = src
        .get(at..line_end)
        .and_then(|rest| rest.find("direction"))
    {
        candidates.push(at + found);
        at += found + 1;
    }
    candidates.into_iter().rev().find_map(|start| {
        let q = skip_spaces(src, start + 9);
        (q > start + 9 && bytes.get(q..q + 2) == Some(pair.as_bytes()))
            .then(|| until(src, q + 2, b'\n'))
    })
}

/// Produces the token stream of Mermaid's class lexer.
pub(crate) fn tokenize(src: &str) -> Vec<Token<'_>> {
    Lexer {
        dir_failed: 0,
        dir_next: src.find("direction"),
        line_end: None,
        p: 0,
        src,
        stack: Vec::new(),
        state: S::Initial,
        tokens: Vec::new(),
    }
    .run()
}

/// The lexer's position and state.
struct Lexer<'a> {
    /// The end of the line on which no direction statement can start.
    dir_failed: usize,
    /// The next `direction` at or after the position, or `None` once there is none.
    dir_next: Option<usize>,
    /// The end of the line holding the position, once found.
    line_end: Option<usize>,
    p: usize,
    src: &'a str,
    stack: Vec<S>,
    state: S,
    tokens: Vec<Token<'a>>,
}

impl<'a> Lexer<'a> {
    /// The byte at `i`.
    fn at(&self, i: usize) -> Option<u8> {
        self.src.as_bytes().get(i).copied()
    }

    /// Emits a token from `s` to `e` and moves to `e`. Always true, as Pele's `emit` is.
    fn emit(&mut self, kind: T, s: usize, e: usize) -> bool {
        self.tokens.push(Token {
            kind,
            start: s,
            text: self.src.get(s..e).unwrap_or(""),
        });
        self.p = e;
        true
    }

    /// Enters `state`, keeping the current one to return to.
    fn push(&mut self, state: S) {
        self.stack.push(self.state);
        self.state = state;
    }

    /// Returns to the previous state, if there is one.
    fn pop(&mut self) {
        if let Some(state) = self.stack.pop() {
            self.state = state;
        }
    }

    /// Whether the ASCII `word` starts at the position, not followed by a word character.
    fn kw(&self, word: &str) -> bool {
        self.src
            .get(self.p..)
            .is_some_and(|rest| rest.starts_with(word))
            && !self
                .at(self.p + word.len())
                .is_some_and(|c| is_word(u32::from(c)))
    }

    /// Whether the ASCII `word` starts at `at`.
    fn starts(&self, at: usize, word: &str) -> bool {
        self.src
            .get(at..)
            .is_some_and(|rest| rest.starts_with(word))
    }

    /// A run of whitespace is a NEWLINE up to its last line feed; the rest is skipped.
    fn space(&mut self, leaves: bool) {
        let mut q = self.p;
        let mut k = None;
        while let Some(c) = char_at(self.src, q).filter(|&c| is_space(c)) {
            if c == 10 {
                k = Some(q);
            }
            q += char::from_u32(c).map_or(1, char::len_utf8);
        }
        let Some(k) = k else {
            self.p = q;
            return;
        };
        if leaves {
            self.pop();
        }
        self.emit(T::Newline, self.p, k + 1);
    }

    /// The keywords that every state recognizes, even inside generics and backtick names.
    fn keyword(&mut self, c: u8) -> bool {
        let p = self.p;
        match c {
            b'c' => {
                if self.kw("cssClass") {
                    return self.emit(T::Cssclass, p, p + 8);
                }
                if self.kw("callback") {
                    return self.emit(T::Callback, p, p + 8);
                }
                if self.kw("click") {
                    return self.emit(T::Click, p, p + 5);
                }
            }
            b'l' if self.kw("link") => return self.emit(T::Link, p, p + 4),
            b'n' => {
                if self.kw("note for") {
                    return self.emit(T::NoteFor, p, p + 8);
                }
                if self.kw("note") {
                    return self.emit(T::Note, p, p + 4);
                }
            }
            b'<' if self.at(p + 1) == Some(b'<') => {
                return self.emit(T::AnnotationStart, p, p + 2);
            }
            b'>' if self.at(p + 1) == Some(b'>') => return self.emit(T::AnnotationEnd, p, p + 2),
            b'h' if self.kw("href") => return self.emit(T::Href, p, p + 4),
            _ => {}
        }
        false
    }

    /// The rules most states share. False when none matches.
    fn common(&mut self, c: u8) -> bool {
        if self.keyword(c) {
            return true;
        }
        let p = self.p;
        let d = self.at(p + 1);
        match c {
            b'"' => {
                self.push(S::String);
                self.p += 1;
                return true;
            }
            b'~' => {
                self.push(S::Generic);
                self.p += 1;
                return true;
            }
            b'`' => {
                self.push(S::Bqstring);
                self.p += 1;
                return true;
            }
            b'_' => {
                for target in ["_self", "_blank", "_parent", "_top"] {
                    if self.kw(target) {
                        return self.emit(T::LinkTarget, p, p + target.len());
                    }
                }
            }
            b'<' => {
                return if d == Some(b'|') {
                    self.emit(T::Extension, p, p + 2)
                } else {
                    self.emit(T::Dependency, p, p + 1)
                };
            }
            b'|' => return d == Some(b'>') && self.emit(T::Extension, p, p + 2),
            b'>' => return self.emit(T::Dependency, p, p + 1),
            b'*' => return self.emit(T::Composition, p, p + 1),
            b'o' if !d.is_some_and(|d| is_word(u32::from(d))) => {
                return self.emit(T::Aggregation, p, p + 1);
            }
            b'(' => return d == Some(b')') && self.emit(T::Lollipop, p, p + 2),
            b'-' => {
                return if d == Some(b'-') {
                    self.emit(T::Line, p, p + 2)
                } else {
                    self.emit(T::Minus, p, p + 1)
                };
            }
            b'.' => {
                return if d == Some(b'.') {
                    self.emit(T::DottedLine, p, p + 2)
                } else {
                    self.emit(T::Dot, p, p + 1)
                };
            }
            b':' => {
                let mut q = p + 1;
                while let Some(ch) = self.at(q) {
                    if ch == b':' || ch == b'\n' || ch == b';' {
                        break;
                    }
                    q += 1;
                }
                if q > p + 1 {
                    return self.emit(T::Label, p, q);
                }
                return d == Some(b':')
                    && self.at(p + 2) == Some(b':')
                    && self.emit(T::StyleSeparator, p, p + 3);
            }
            b'+' => return self.emit(T::Plus, p, p + 1),
            b'%' => return self.emit(T::Pct, p, p + 1),
            b'=' => return self.emit(T::Equals, p, p + 1),
            b'[' => return self.emit(T::Sqs, p, p + 1),
            b']' => return self.emit(T::Sqe, p, p + 1),
            b'!' | b'#' | b'$' | b'&' | b'\'' | b',' | b'?' | b'\\' | b'/' => {
                return self.emit(T::Punctuation, p, p + 1);
            }
            _ => {}
        }
        if is_word(u32::from(c)) {
            let mut q = p + 1;
            while self.at(q).is_some_and(|c| is_word(u32::from(c))) {
                q += 1;
            }
            return self.emit(T::Alpha, p, q);
        }
        match char::from_u32(char_at(self.src, p).unwrap_or(0)) {
            Some(ch) if is_unicode_letter(u32::from(ch)) => {
                self.emit(T::UnicodeText, p, p + ch.len_utf8())
            }
            _ => false,
        }
    }

    /// A direction statement swallows the rest of its line, from wherever the lexer stands on it.
    fn direction(&mut self) -> bool {
        let p = self.p;
        let Some(mut next) = self.dir_next else {
            return false;
        };
        if p < self.dir_failed {
            return false;
        }
        if next < p {
            let Some(found) = self.src.get(p..).and_then(|rest| rest.find("direction")) else {
                self.dir_next = None;
                return false;
            };
            next = p + found;
            self.dir_next = Some(next);
        }
        let line_end = match self.line_end {
            Some(end) if end >= p => end,
            _ => {
                let end = self
                    .src
                    .get(p..)
                    .and_then(|rest| rest.find(|c: char| is_line_end(u32::from(c))))
                    .map_or(self.src.len(), |at| p + at);
                self.line_end = Some(end);
                end
            }
        };
        if next >= line_end {
            return false;
        }
        for (k, pair) in ["TB", "BT", "RL", "LR"].into_iter().enumerate() {
            if let Some(end) = direction_end(self.src, p, line_end, pair) {
                let kind = [
                    T::DirectionTb,
                    T::DirectionBt,
                    T::DirectionRl,
                    T::DirectionLr,
                ]
                .get(k)
                .copied()
                .unwrap_or(T::DirectionTb);
                return self.emit(kind, p, end);
            }
        }
        // No direction statement starts anywhere in the rest of this line either.
        self.dir_failed = line_end;
        false
    }

    /// The rules of the initial state. False when none matches.
    fn initial(&mut self, c: u8) -> bool {
        if self.direction() {
            return true;
        }
        let p = self.p;
        if space_at(self.src, p) {
            self.space(false);
            return true;
        }
        match c {
            b'%' if self.at(p + 1) == Some(b'%') => {
                let mut e = until(self.src, p, b'\n');
                while matches!(self.at(e), Some(b'\n' | b'\r')) {
                    e += 1;
                }
                self.p = e;
                return true;
            }
            b'a' => {
                let title = self.starts(p, "accTitle");
                if title || self.starts(p, "accDescr") {
                    let q = skip_spaces(self.src, p + 8);
                    match self.at(q) {
                        Some(b':') => {
                            let e = skip_spaces(self.src, q + 1);
                            self.emit(if title { T::AccTitle } else { T::AccDescr }, p, e);
                            self.push(if title { S::AccTitle } else { S::AccDescr });
                            return true;
                        }
                        Some(b'{') if !title => {
                            self.p = skip_spaces(self.src, q + 1);
                            self.push(S::AccDescrMultiline);
                            return true;
                        }
                        _ => {}
                    }
                }
            }
            b'c' => {
                if self.kw("classDiagram-v2") {
                    return self.emit(T::ClassDiagram, p, p + 15);
                }
                if self.kw("classDiagram") {
                    return self.emit(T::ClassDiagram, p, p + 12);
                }
                if self.starts(p, "call") && space_at(self.src, p + 4) {
                    self.p = skip_spaces(self.src, p + 4);
                    self.push(S::CallbackName);
                    return true;
                }
                if self.kw("classDef") {
                    return self.emit(T::Classdef, p, p + 8);
                }
                if self.kw("class") {
                    self.push(S::Cls);
                    return self.emit(T::Class, p, p + 5);
                }
            }
            b'[' if self.starts(p + 1, "*]") => return self.emit(T::EdgeState, p, p + 3),
            b's' if self.kw("style") => return self.emit(T::Style, p, p + 5),
            b'n' if self.kw("namespace") => {
                self.push(S::Namespace);
                return self.emit(T::Namespace, p, p + 9);
            }
            b',' => return self.emit(T::Comma, p, p + 1),
            b'#' => return self.emit(T::Brkt, p, p + 1),
            _ => {}
        }
        self.common(c) || (c == b':' && self.emit(T::Colon, p, p + 1))
    }

    /// Reads the whole source.
    #[expect(clippy::too_many_lines, reason = "mirrors Pele's tokenize")]
    fn run(mut self) -> Vec<Token<'a>> {
        let n = self.src.len();
        loop {
            let p = self.p;
            let Some(c) = self.at(p) else {
                let kind = at_end(self.state);
                self.emit(kind, n, n);
                break;
            };
            let before = (p, self.state, self.stack.len());
            let ok = match self.state {
                S::Initial => self.initial(c),
                S::Cls => {
                    if space_at(self.src, p) {
                        self.space(true);
                        true
                    } else if c == b'}' {
                        self.pop();
                        self.pop();
                        self.emit(T::StructStop, p, p + 1)
                    } else if c == b'{' {
                        self.push(S::ClassBody);
                        self.emit(T::StructStart, p, p + 1)
                    } else {
                        self.common(c)
                    }
                }
                S::ClassBody => {
                    if c == b'"' {
                        self.push(S::String);
                        self.p += 1;
                    } else if c == b'}' {
                        self.pop();
                        self.emit(T::StructStop, p, p + 1);
                    } else if c == b'[' && self.starts(p + 1, "*]") {
                        self.emit(T::EdgeState, p, p + 3);
                    } else if c == b'{' {
                        self.emit(T::OpenInStruct, p, p + 1);
                    } else if c == b'\n' {
                        self.p += 1;
                    } else {
                        let mut q = p + 1;
                        while let Some(ch) = self.at(q) {
                            if ch == b'{' || ch == b'}' || ch == b'\n' {
                                break;
                            }
                            q += 1;
                        }
                        self.emit(T::Member, p, q);
                    }
                    true
                }
                S::Namespace => {
                    if space_at(self.src, p) {
                        self.space(true);
                        true
                    } else if c == b'{' {
                        self.push(S::NamespaceBody);
                        self.emit(T::StructStart, p, p + 1)
                    } else if c == b'}' {
                        // The brace is read again in the state the namespace was opened from.
                        self.pop();
                        true
                    } else if c == b'n' && self.kw("namespace") {
                        self.push(S::Namespace);
                        self.emit(T::Namespace, p, p + 9)
                    } else {
                        self.common(c)
                    }
                }
                S::NamespaceBody => {
                    if space_at(self.src, p) {
                        self.space(false);
                        true
                    } else if c == b'}' {
                        self.pop();
                        self.emit(T::StructStop, p, p + 1)
                    } else if c == b'n' && self.kw("namespace") {
                        self.push(S::Namespace);
                        self.emit(T::Namespace, p, p + 9)
                    } else if c == b'[' && self.starts(p + 1, "*]") {
                        self.emit(T::EdgeState, p, p + 3)
                    } else if c == b'c' && self.kw("class") {
                        self.push(S::Cls);
                        self.emit(T::Class, p, p + 5)
                    } else {
                        self.common(c)
                    }
                }
                S::String => {
                    if c == b'"' {
                        self.pop();
                        self.p += 1;
                    } else {
                        let e = until(self.src, p, b'"');
                        self.emit(T::Str, p, e);
                    }
                    true
                }
                S::Generic => {
                    if c == b'"' {
                        self.push(S::String);
                        self.p += 1;
                    } else if self.keyword(c) {
                    } else if c == b'~' {
                        self.pop();
                        self.p += 1;
                    } else {
                        let e = until(self.src, p, b'~');
                        self.emit(T::Generictype, p, e);
                    }
                    true
                }
                S::Bqstring => {
                    if c == b'"' {
                        self.push(S::String);
                        self.p += 1;
                    } else if self.keyword(c) {
                    } else if c == b'~' {
                        self.push(S::Generic);
                        self.p += 1;
                    } else if c == b'`' {
                        self.pop();
                        self.p += 1;
                    } else {
                        let e = until(self.src, p, b'`');
                        self.emit(T::BquoteStr, p, e);
                    }
                    true
                }
                S::CallbackName => {
                    if c == b'(' {
                        let q = skip_spaces(self.src, p + 1);
                        self.pop();
                        if self.at(q) == Some(b')') {
                            self.p = q + 1;
                        } else {
                            self.push(S::CallbackArgs);
                            self.p += 1;
                        }
                    } else {
                        let e = until(self.src, p, b'(');
                        self.emit(T::CallbackName, p, e);
                    }
                    true
                }
                S::CallbackArgs => {
                    if c == b')' {
                        self.pop();
                        self.p += 1;
                    } else {
                        let e = until(self.src, p, b')');
                        self.emit(T::CallbackArgs, p, e);
                    }
                    true
                }
                S::AccTitle | S::AccDescr => {
                    let kind = if self.state == S::AccTitle {
                        T::AccTitleValue
                    } else {
                        T::AccDescrValue
                    };
                    let e = until(self.src, p, b'\n');
                    self.emit(kind, p, e);
                    self.pop();
                    true
                }
                S::AccDescrMultiline => {
                    if c == b'}' {
                        self.pop();
                        self.p += 1;
                    } else {
                        let e = until(self.src, p, b'}');
                        self.emit(T::AccDescrMultilineValue, p, e);
                    }
                    true
                }
            };

            if !ok {
                self.tokens.push(Token {
                    kind: T::Error,
                    start: self.p,
                    text: "",
                });
                return self.tokens;
            }
            // Every step reads input or changes state. A namespace closing with nothing below it
            // would do neither, and is reported as unrecognized text rather than looping.
            if (self.p, self.state, self.stack.len()) == before {
                self.tokens.push(Token {
                    kind: T::Error,
                    start: self.p,
                    text: "",
                });
                return self.tokens;
            }
        }

        self.tokens.push(Token {
            kind: T::End,
            start: n,
            text: "",
        });
        self.tokens
    }
}
