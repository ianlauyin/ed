//! The block lexer: a state machine equivalent to Mermaid's Jison lexer. Port of Pele's
//! `src/diagrams/block/lexer.ts`.
//!
//! Pele reads UTF-16 code units. This port reads UTF-8 bytes: every delimiter is ASCII, and a
//! non-ASCII character is decoded whole wherever `\s` looks at it, so token boundaries match.

use super::tokens::T;
use crate::util::chars::{is_space, is_word};

/// A token: its kind, its start as a UTF-8 offset, and its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token<'a> {
    /// The token kind.
    pub(crate) kind: T,
    /// The UTF-8 offset of the token in the source.
    pub(crate) start: usize,
    /// The token text. Some tokens carry a fixed text in place of the matched one.
    pub(crate) text: &'a str,
}

/// Lexer start conditions, as in Mermaid's grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum S {
    AccDescr,
    AccDescrMultiline,
    AccTitle,
    ArrowDir,
    BlockArrow,
    Class,
    ClassDef,
    ClassDefId,
    ClassStyle,
    Initial,
    LLabel,
    MdString,
    Node,
    String,
    StyleDefinition,
    StyleStmnt,
}

/// The code past the end of the source, which no test matches. Pele's `charCodeAt` returns `NaN`.
const NONE: u32 = u32::MAX;

/// Block arrow directions, in the order Pele tries them.
const DIRECTIONS: [&str; 6] = ["right", "left", "x", "y", "up", "down"];

/// Whether an ASCII character ends a node id: `( ) [ { } < > - : =` and whitespace.
fn id_stop(c: u32) -> bool {
    matches!(
        c,
        9 | 10 | 11 | 12 | 13 | 32 | 40 | 41 | 45 | 58 | 60 | 61 | 62 | 91 | 123 | 125
    )
}

/// Lexer state. Pele keeps these as closure variables of `tokenize`.
struct Lexer<'a> {
    p: usize,
    src: &'a str,
    stack: Vec<S>,
    state: S,
    tokens: Vec<Token<'a>>,
}

impl<'a> Lexer<'a> {
    /// The character at `at`, decoded whole when it is not ASCII, or [`NONE`] past the end.
    fn code(&self, at: usize) -> u32 {
        let Some(&b) = self.src.as_bytes().get(at) else {
            return NONE;
        };
        if b < 0x80 {
            return u32::from(b);
        }
        self.src
            .get(at..)
            .and_then(|rest| rest.chars().next())
            .map_or(u32::from(b), u32::from)
    }

    /// The offset after the character at `at`.
    fn after(&self, at: usize) -> usize {
        let len = self
            .src
            .get(at..)
            .and_then(|rest| rest.chars().next())
            .map_or(1, char::len_utf8);
        at + len
    }

    fn starts_with(&self, text: &str, at: usize) -> bool {
        self.src
            .as_bytes()
            .get(at..)
            .is_some_and(|rest| rest.starts_with(text.as_bytes()))
    }

    fn slice(&self, from: usize, to: usize) -> &'a str {
        self.src.get(from..to).unwrap_or("")
    }

    fn put(&mut self, kind: T, text: &'a str, end: usize) -> bool {
        self.tokens.push(Token {
            kind,
            start: self.p,
            text,
        });
        self.p = end;
        true
    }

    fn emit(&mut self, kind: T, end: usize) -> bool {
        let text = self.slice(self.p, end);
        self.put(kind, text, end)
    }

    fn push(&mut self, s: S) {
        self.stack.push(self.state);
        self.state = s;
    }

    fn pop(&mut self) {
        if let Some(s) = self.stack.pop() {
            self.state = s;
        }
    }

    fn ws(&self, mut q: usize) -> usize {
        while q < self.src.len() && is_space(self.code(q)) {
            q = self.after(q);
        }
        q
    }

    fn digits(&self, mut q: usize) -> usize {
        while self.src.as_bytes().get(q).is_some_and(u8::is_ascii_digit) {
            q += 1;
        }
        q
    }

    fn word(&self, mut q: usize) -> usize {
        while is_word(self.code(q)) {
            q += 1;
        }
        q
    }

    fn keyword(&self, text: &str) -> bool {
        self.starts_with(text, self.p) && !is_word(self.code(self.p + text.len()))
    }

    fn quote(&mut self) -> bool {
        if self.code(self.p + 1) == 96 {
            self.push(S::MdString);
            self.p += 2;
        } else {
            self.push(S::String);
            self.p += 1;
        }
        true
    }

    /// The end of `.+-[xo>]?` at `q`, if there is one.
    fn dotted(&self, mut q: usize) -> Option<usize> {
        if self.code(q) != 46 {
            return None;
        }
        q += 1;
        while self.code(q) == 46 {
            q += 1;
        }
        if self.code(q) != 45 {
            return None;
        }
        Some(if matches!(self.code(q + 1), 120 | 111 | 62) {
            q + 2
        } else {
            q + 1
        })
    }

    /// The end of a complete link at `q`, before its trailing whitespace, if there is one.
    fn link_end(&self, mut q: usize) -> Option<usize> {
        if matches!(self.code(q), 120 | 111 | 60) {
            q += 1;
        }
        let line = self.code(q);
        if line == 46 {
            return self.dotted(q);
        }
        if line != 45 && line != 61 {
            return None;
        }
        let mut k = q + 1;
        while self.code(k) == line {
            k += 1;
        }
        if k - q == 1 {
            return if line == 45 { self.dotted(k) } else { None };
        }
        if matches!(self.code(k), 120 | 111 | 62) {
            return Some(k + 1);
        }
        (k - q >= 3).then_some(k)
    }

    fn link(&mut self) -> bool {
        if let Some(e) = self.link_end(self.p) {
            let end = self.ws(e);
            return self.emit(T::Link, end);
        }
        let q = if self.code(self.p) == 60 {
            self.p + 1
        } else {
            self.p
        };
        let a = self.code(q);
        let b = self.code(q + 1);
        if (a == 45 && (b == 45 || b == 46)) || (a == 61 && b == 61) {
            self.push(S::LLabel);
            let end = self.ws(q + 2);
            return self.emit(T::StartLink, end);
        }
        false
    }

    fn line_state(&mut self, kind: T) -> bool {
        let e = self
            .src
            .get(self.p..)
            .and_then(|rest| rest.find('\n'))
            .map_or(self.src.len(), |at| self.p + at);
        self.pop();
        self.emit(kind, e)
    }

    /// `\w+(,\s*\w+)*`
    fn id_list(&mut self, kind: T, next: S) -> bool {
        let mut k = self.word(self.p);
        if k == self.p {
            return false;
        }
        while self.code(k) == 44 {
            let from = self.ws(k + 1);
            let to = self.word(from);
            if to == from {
                break;
            }
            k = to;
        }
        self.state = next;
        self.emit(kind, k)
    }

    #[expect(clippy::too_many_lines, reason = "mirrors Pele's initial")]
    fn initial(&mut self, c: u32) -> bool {
        let p = self.p;
        if is_space(c) {
            self.p = self.ws(self.after(p));
            return true;
        }
        match c {
            98 => {
                if self.starts_with("block", p) {
                    if self.keyword("block-beta") {
                        return self.emit(T::Key, p + 10);
                    }
                    if self.code(p + 5) == 58 {
                        return self.emit(T::IdBlock, p + 6);
                    }
                    if !is_word(self.code(p + 5)) {
                        return self.emit(T::Key, p + 5);
                    }
                }
            }
            99 => {
                if self.starts_with("columns", p) {
                    let q = self.ws(p + 7);
                    if q > p + 7 {
                        if self.starts_with("auto", q) && !is_word(self.code(q + 4)) {
                            return self.put(T::Columns, "-1", q + 4);
                        }
                        let e = self.digits(q);
                        if e > q {
                            let text = self.slice(q, e);
                            return self.put(T::Columns, text, e);
                        }
                    }
                } else if self.starts_with("class", p) {
                    let def = self.starts_with("Def", p + 5);
                    let from = if def { p + 8 } else { p + 5 };
                    let q = self.ws(from);
                    if q > from {
                        self.push(if def { S::ClassDef } else { S::Class });
                        return self.emit(if def { T::ClassDef } else { T::Class }, q);
                    }
                }
            }
            34 => return self.quote(),
            115 => {
                if self.starts_with("space", p) {
                    if self.code(p + 5) == 58 {
                        let e = self.digits(p + 6);
                        if e > p + 6 {
                            let text = self.slice(p + 6, e);
                            return self.put(T::SpaceBlock, text, e);
                        }
                    }
                    if !is_word(self.code(p + 5)) {
                        return self.put(T::SpaceBlock, "1", p + 5);
                    }
                } else if self.starts_with("style", p) {
                    let q = self.ws(p + 5);
                    if q > p + 5 {
                        self.push(S::StyleStmnt);
                        return self.emit(T::Style, q);
                    }
                }
            }
            100 => {
                if self.keyword("default") {
                    return self.emit(T::Default, p + 7);
                }
            }
            108 => {
                if self.keyword("linkStyle") {
                    return self.emit(T::LinkStyle, p + 9);
                }
            }
            105 => {
                if self.keyword("interpolate") {
                    return self.emit(T::Interpolate, p + 11);
                }
            }
            97 => {
                if self.starts_with("accTitle", p) {
                    let q = self.ws(p + 8);
                    if self.code(q) == 58 {
                        self.push(S::AccTitle);
                        let end = self.ws(q + 1);
                        return self.emit(T::AccTitle, end);
                    }
                } else if self.starts_with("accDescr", p) {
                    let q = self.ws(p + 8);
                    let d = self.code(q);
                    if d == 58 {
                        self.push(S::AccDescr);
                        let end = self.ws(q + 1);
                        return self.emit(T::AccDescr, end);
                    }
                    if d == 123 {
                        self.push(S::AccDescrMultiline);
                        self.p = self.ws(q + 1);
                        return true;
                    }
                }
            }
            101 => {
                if self.keyword("end") {
                    let end = self.ws(p + 3);
                    return self.emit(T::EndKeyword, end);
                }
            }
            45 => {
                if self.code(p + 1) == 41 {
                    self.push(S::Node);
                    return self.emit(T::NodeDStart, p + 2);
                }
                return self.link();
            }
            61 => return self.link(),
            60 => {
                if self.code(p + 1) == 91 {
                    self.push(S::BlockArrow);
                    return self.emit(T::BlockArrowStart, p + 2);
                }
                return self.link();
            }
            40 => {
                let d = self.code(p + 1);
                let len = match d {
                    40 if self.code(p + 2) == 40 => 3,
                    40 | 45 | 91 => 2,
                    _ => 1,
                };
                self.push(S::Node);
                return self.emit(T::NodeDStart, p + len);
            }
            41 | 123 => {
                self.push(S::Node);
                let end = if self.code(p + 1) == c { p + 2 } else { p + 1 };
                return self.emit(T::NodeDStart, end);
            }
            62 => {
                self.push(S::Node);
                return self.emit(T::NodeDStart, p + 1);
            }
            91 => {
                let d = self.code(p + 1);
                self.push(S::Node);
                let end = if matches!(d, 91 | 124 | 40 | 92 | 47) {
                    p + 2
                } else {
                    p + 1
                };
                return self.emit(T::NodeDStart, end);
            }
            58 => {
                let e = self.digits(p + 1);
                if e > p + 1 {
                    let text = self.slice(p + 1, e);
                    return self.put(T::Size, text, e);
                }
                return false;
            }
            _ => {}
        }

        let mut q = p;
        while q < self.src.len() {
            let ch = self.code(q);
            if if ch < 128 { id_stop(ch) } else { is_space(ch) } {
                break;
            }
            q = self.after(q);
        }
        q > p && self.emit(T::NodeId, q)
    }

    fn node_end(&mut self, c: u32) -> bool {
        let p = self.p;
        let d = self.code(p + 1);
        let len = match c {
            34 => return self.quote(),
            40 => match d {
                40 if self.code(p + 2) == 40 => 3,
                40 | 45 => 2,
                _ => 1,
            },
            41 => match d {
                41 if self.code(p + 2) == 41 => 3,
                41 | 93 => 2,
                _ => 1,
            },
            125 => {
                if d == 125 {
                    2
                } else {
                    1
                }
            }
            45 if d == 41 => 2,
            93 => {
                if matches!(d, 93 | 41 | 62) {
                    2
                } else {
                    1
                }
            }
            92 | 47 if d == 93 => 2,
            _ => 0,
        };
        if len == 0 {
            return false;
        }
        self.pop();
        self.emit(T::NodeDEnd, p + len)
    }

    fn arrow_dir(&mut self, c: u32) -> bool {
        let p = self.p;
        if c == 41 {
            self.pop();
            self.pop();
            let end = self.ws(p + 1);
            return self.put(T::BlockArrowEnd, "]>", end);
        }
        let from = self.ws(if c == 44 { p + 1 } else { p });
        for dir in DIRECTIONS {
            if self.starts_with(dir, from) {
                let e = self.ws(from + dir.len());
                let text = self.slice(if c == 44 { from } else { p }, e);
                return self.put(T::Dir, text, e);
            }
        }
        false
    }

    /// The token kind a state yields at the end of input, if any.
    fn final_kind(&self) -> Option<T> {
        match self.state {
            S::Initial => Some(T::Eof),
            S::String => Some(T::Str),
            S::ClassDefId => Some(T::ClassDefStyleOpts),
            S::ClassStyle => Some(T::StyleClass),
            S::StyleDefinition => Some(T::StyleDefinitionData),
            S::AccTitle => Some(T::AccTitleValue),
            S::AccDescr => Some(T::AccDescrValue),
            S::AccDescrMultiline => Some(T::AccDescrMultilineValue),
            _ => None,
        }
    }

    /// The end of text up to the next `needle` from the current position, or the end of input.
    fn until(&self, needle: char) -> usize {
        self.src
            .get(self.p..)
            .and_then(|rest| rest.find(needle))
            .map_or(self.src.len(), |at| self.p + at)
    }

    #[expect(clippy::too_many_lines, reason = "mirrors Pele's tokenize loop")]
    fn step(&mut self, c: u32) -> bool {
        let p = self.p;
        match self.state {
            S::Initial => self.initial(c),
            S::String => {
                if c == 34 {
                    self.pop();
                    self.p += 1;
                } else {
                    let e = self.until('"');
                    self.emit(T::Str, e);
                }
                true
            }
            S::MdString => {
                if c == 96 {
                    let ok = self.code(p + 1) == 34;
                    if ok {
                        self.pop();
                        self.p += 2;
                    }
                    ok
                } else if c == 34 {
                    false
                } else {
                    let mut q = self.after(p);
                    while q < self.src.len() {
                        let ch = self.code(q);
                        if ch == 96 || ch == 34 {
                            break;
                        }
                        q = self.after(q);
                    }
                    self.emit(T::MdStr, q)
                }
            }
            S::Node => self.node_end(c),
            S::BlockArrow => {
                if c == 34 {
                    self.quote()
                } else if c == 93 && self.code(p + 1) == 62 {
                    let q = self.ws(p + 2);
                    let ok = self.code(q) == 40;
                    if ok {
                        self.push(S::ArrowDir);
                        self.p = q + 1;
                    }
                    ok
                } else {
                    false
                }
            }
            S::ArrowDir => self.arrow_dir(c),
            S::LLabel => {
                if c == 34 {
                    if self.code(p + 1) == 96 {
                        self.quote();
                    } else {
                        self.emit(T::LinkLabel, p + 1);
                        self.push(S::String);
                    }
                    true
                } else {
                    let from = self.ws(p);
                    match self.link_end(from) {
                        Some(e) => {
                            self.pop();
                            let end = self.ws(e);
                            self.emit(T::Link, end)
                        }
                        None => false,
                    }
                }
            }
            S::ClassDef => {
                let mut kind = T::DefaultClassDefId;
                let mut k = if self.starts_with("DEFAULT", p) {
                    p + 7
                } else {
                    p
                };
                let mut q = self.ws(k);
                if k == p || q == k {
                    kind = T::ClassDefId;
                    k = self.word(p);
                    q = self.ws(k);
                }
                let ok = k > p && q > k;
                if ok {
                    self.state = S::ClassDefId;
                    self.emit(kind, q);
                }
                ok
            }
            S::ClassDefId => self.line_state(T::ClassDefStyleOpts),
            S::Class => self.id_list(T::ClassEntityIds, S::ClassStyle),
            S::ClassStyle => self.line_state(T::StyleClass),
            S::StyleStmnt => self.id_list(T::StyleEntityIds, S::StyleDefinition),
            S::StyleDefinition => self.line_state(T::StyleDefinitionData),
            S::AccTitle => self.line_state(T::AccTitleValue),
            S::AccDescr => self.line_state(T::AccDescrValue),
            S::AccDescrMultiline => {
                if c == 125 {
                    self.pop();
                    self.p += 1;
                } else {
                    let e = self.until('}');
                    self.emit(T::AccDescrMultilineValue, e);
                }
                true
            }
        }
    }
}

/// Splits `src` into tokens. The list ends with [`T::End`], or with [`T::Error`] at unrecognized
/// text.
pub(crate) fn tokenize(src: &str) -> Vec<Token<'_>> {
    let mut lexer = Lexer {
        p: 0,
        src,
        stack: Vec::new(),
        state: S::Initial,
        tokens: Vec::new(),
    };
    let n = src.len();

    loop {
        if lexer.p >= n {
            lexer.p = n;
            if let Some(kind) = lexer.final_kind() {
                lexer.put(kind, "", n);
            }
            break;
        }

        let c = lexer.code(lexer.p);
        if !lexer.step(c) {
            lexer.tokens.push(Token {
                kind: T::Error,
                start: lexer.p,
                text: "",
            });
            return lexer.tokens;
        }
    }

    lexer.tokens.push(Token {
        kind: T::End,
        start: n,
        text: "",
    });
    lexer.tokens
}
