//! The sequence lexer, a state machine over Mermaid's lexer rules. Port of Pele's
//! `src/diagrams/sequence/lexer.ts`.
//!
//! Pele reads UTF-16 code units. This port reads UTF-8 bytes: every delimiter is ASCII, and a
//! non-ASCII character is decoded whole wherever a rule looks at it, so token boundaries match.

use super::tokens::T;
use crate::text::label::trim;
use crate::util::chars::{is_digit, is_space, is_word};

/// A token: its kind, its byte offset, and its text.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Token<'a> {
    /// The kind.
    pub(crate) kind: T,
    /// The byte offset in the source.
    pub(crate) start: usize,
    /// The text, trimmed for participant names.
    pub(crate) text: &'a str,
}

/// A lexer state, as Mermaid's lexer names its start conditions.
#[derive(Clone, Copy, PartialEq, Eq)]
enum S {
    Initial,
    Id,
    Alias,
    Line,
    Config,
    AccTitle,
    AccDescr,
    AccDescrMultiline,
}

const ACTOR_START: u8 = 1;
const ACTOR_REST: u8 = 2;
const ID: u8 = 4;
const ID_WIDE: u8 = 8;
const LINE: u8 = 16;
const MENU: u8 = 32;

/// Character classes of Mermaid's lexer rules, for ASCII. Other characters are tested by `\s`.
const CLASS: [u8; 128] = {
    let mut table = [0u8; 128];
    let mut c = 0;
    while c < 128 {
        let ws = c == 32 || (c >= 9 && c <= 13);
        let mut bits = 0;
        if !in_set(c, b"/\\+()<->:\n,;") {
            bits |= ACTOR_START;
        }
        if !in_set(c, b"+<->:\n,;") {
            bits |= ACTOR_REST;
        }
        if !in_set(c, b"<>:\n,;@") {
            bits |= ID_WIDE | if ws { 0 } else { ID };
        }
        if !in_set(c, b"#\n;") {
            bits |= LINE;
        }
        if !in_set(c, b"/\\+()<>:\n,;-") && !ws {
            bits |= MENU;
        }
        table[c] = bits;
        c += 1;
    }
    table
};

const fn in_set(c: usize, set: &[u8]) -> bool {
    let mut i = 0;
    while i < set.len() {
        if set[i] as usize == c {
            return true;
        }
        i += 1;
    }
    false
}

/// What `charCodeAt` returns past the end. It is no character, so every test of it fails.
const NONE: u32 = u32::MAX;

struct Lexer<'a> {
    bytes: &'a [u8],
    n: usize,
    next_nl: Option<usize>,
    p: usize,
    src: &'a str,
    stack: Vec<S>,
    state: S,
    tokens: Vec<Token<'a>>,
}

impl<'a> Lexer<'a> {
    /// The character at byte `q`, or [`NONE`] past the end.
    fn at(&self, q: usize) -> u32 {
        match self.bytes.get(q) {
            None => NONE,
            Some(&b) if b < 0x80 => u32::from(b),
            Some(&b) => self
                .src
                .get(q..)
                .and_then(|rest| rest.chars().next())
                .map_or(u32::from(b), u32::from),
        }
    }

    /// The byte length of the character at `q`.
    fn len(&self, q: usize) -> usize {
        match self.bytes.get(q) {
            Some(&b) if b >= 0x80 => self
                .src
                .get(q..)
                .and_then(|rest| rest.chars().next())
                .map_or(1, char::len_utf8),
            _ => 1,
        }
    }

    fn slice(&self, s: usize, e: usize) -> &'a str {
        self.src.get(s..e).unwrap_or("")
    }

    fn emit(&mut self, kind: T, s: usize, e: usize) {
        let text = self.slice(s, e);
        self.emit_text(kind, s, e, text);
    }

    fn emit_text(&mut self, kind: T, s: usize, e: usize, text: &'a str) {
        self.tokens.push(Token {
            kind,
            start: s,
            text,
        });
        self.p = e;
    }

    fn push(&mut self, next: S) {
        self.stack.push(self.state);
        self.state = next;
    }

    fn pop(&mut self) {
        if let Some(state) = self.stack.pop() {
            self.state = state;
        }
    }

    fn eol(&mut self, q: usize) -> usize {
        if self.next_nl.is_none_or(|nl| nl < q) {
            let found = self
                .bytes
                .get(q..)
                .and_then(|rest| rest.iter().position(|&b| b == b'\n'));
            self.next_nl = Some(found.map_or(self.n, |at| q + at));
        }
        self.next_nl.unwrap_or(self.n)
    }

    fn has(&self, q: usize, bit: u8) -> bool {
        let c = self.at(q);
        if let Some(&class) = usize::try_from(c).ok().and_then(|c| CLASS.get(c)) {
            return class & bit != 0;
        }
        if bit == ID || bit == MENU {
            !is_space(c)
        } else {
            true
        }
    }

    /// Matches a lowercase literal without regard to the case of the source.
    fn ci(&self, q: usize, word: &str) -> bool {
        word.bytes().enumerate().all(|(k, w)| {
            let Some(&c) = self.bytes.get(q + k) else {
                return false;
            };
            if w.is_ascii_lowercase() {
                c | 32 == w
            } else {
                c == w
            }
        })
    }

    fn word(&self, lit: &str) -> bool {
        self.ci(self.p, lit) && !is_word(self.at(self.p + lit.len()))
    }

    fn skip_ws(&self, mut q: usize) -> usize {
        while q < self.n && is_space(self.at(q)) {
            q += self.len(q);
        }
        q
    }

    fn skip_same_line(&mut self) -> bool {
        let mut q = self.p;
        while q < self.n {
            let c = self.at(q);
            if c == 10 || !is_space(c) {
                break;
            }
            q += self.len(q);
        }
        if q == self.p {
            return false;
        }
        self.p = q;
        true
    }

    fn line_run(&self, mut q: usize) -> usize {
        while q < self.n && self.has(q, LINE) {
            q += self.len(q);
        }
        q
    }

    fn begin(&mut self, kind: T, len: usize, next: S) {
        self.emit(kind, self.p, self.p + len);
        self.push(next);
    }

    fn keyword(&mut self, lit: &str, kind: T, next: Option<S>) -> bool {
        if !self.word(lit) {
            return false;
        }
        self.emit(kind, self.p, self.p + lit.len());
        if let Some(next) = next {
            self.push(next);
        }
        true
    }

    /// The actor menu keywords count only when spaces and the start of a name follow.
    fn menu(&mut self, lit: &str, kind: T) -> bool {
        if !self.ci(self.p, lit) {
            return false;
        }
        let mut q = self.p + lit.len();
        let mut c = self.at(q);
        if c != 32 && c != 9 {
            return false;
        }
        while c == 32 || c == 9 {
            q += 1;
            c = self.at(q);
        }
        if q >= self.n || !self.has(q, MENU) {
            return false;
        }
        self.emit(kind, self.p, self.p + lit.len());
        true
    }

    fn arrow_ahead(&self, q: usize) -> bool {
        let c = self.at(q);
        let d = self.at(q + 1);
        if c == 45 {
            if matches!(d, 120 | 88 | 41 | 92 | 47 | 45) {
                return true;
            }
            let e = self.at(q + 2);
            return d == 124 && (e == 92 || e == 47);
        }
        if c == 47 || c == 92 {
            return (d == 124 || d == c) && self.at(q + 2) == 45;
        }
        c == 40 && d == 41
    }

    fn actor(&mut self) {
        let p = self.p;
        let mut q = p + self.len(p);
        while q < self.n && self.has(q, ACTOR_START) {
            q += self.len(q);
        }
        while q < self.n && !self.arrow_ahead(q) {
            let mut r = q;
            while self.at(r) == 45 {
                r += 1;
            }
            if r >= self.n || !self.has(r, ACTOR_REST) {
                break;
            }
            r += self.len(r);
            while r < self.n && self.has(r, ACTOR_REST) {
                r += self.len(r);
            }
            q = r;
        }
        let text = trim(self.slice(p, q));
        self.emit_text(T::Actor, p, q, text);
    }

    fn number(&mut self) -> bool {
        let p = self.p;
        let mut q = p;
        while is_digit(self.at(q)) {
            q += 1;
        }
        let whole = q > p;
        if self.at(q) == 46 && is_digit(self.at(q + 1)) {
            let r = if is_digit(self.at(q + 2)) {
                q + 3
            } else {
                q + 2
            };
            let after = self.at(r);
            if after == 32 || after == 10 {
                self.emit(T::Num, p, r);
                return true;
            }
        }
        let after = self.at(q);
        if whole && (after == 32 || after == 10) {
            self.emit(T::Num, p, q);
            return true;
        }
        false
    }

    fn dash(&mut self) {
        let p = self.p;
        let d = self.at(p + 1);
        let e = self.at(p + 2);
        let (kind, len) = match (d, e) {
            (62, 62) => (T::SolidArrow, 3),
            (62, _) => (T::SolidOpenArrow, 2),
            (120 | 88, _) => (T::SolidCross, 2),
            (41, _) => (T::SolidPoint, 2),
            (124, 92) => (T::SolidArrowTop, 3),
            (124, 47) => (T::SolidArrowBottom, 3),
            (92, 92) => (T::StickArrowTop, 3),
            (47, 47) => (T::StickArrowBottom, 3),
            (45, _) => {
                let f = self.at(p + 3);
                match (e, f) {
                    (62, 62) => (T::DottedArrow, 4),
                    (62, _) => (T::DottedOpenArrow, 3),
                    (120 | 88, _) => (T::DottedCross, 3),
                    (41, _) => (T::DottedPoint, 3),
                    (124, 92) => (T::SolidArrowTopDotted, 4),
                    (124, 47) => (T::SolidArrowBottomDotted, 4),
                    (92, 92) => (T::StickArrowTopDotted, 4),
                    (47, 47) => (T::StickArrowBottomDotted, 4),
                    _ => (T::Minus, 1),
                }
            }
            _ => (T::Minus, 1),
        };
        self.emit(kind, p, p + len);
    }

    fn reverse(&mut self, c: u32) -> bool {
        let p = self.p;
        let d = self.at(p + 1);
        if (d != 124 && d != c) || self.at(p + 2) != 45 {
            return false;
        }
        let dotted = self.at(p + 3) == 45;
        let top = c == 47;
        let kind = match (d == 124, dotted, top) {
            (true, true, true) => T::SolidArrowTopReverseDotted,
            (true, true, false) => T::SolidArrowBottomReverseDotted,
            (true, false, true) => T::SolidArrowTopReverse,
            (true, false, false) => T::SolidArrowBottomReverse,
            (false, true, true) => T::StickArrowTopReverseDotted,
            (false, true, false) => T::StickArrowBottomReverseDotted,
            (false, false, true) => T::StickArrowTopReverse,
            (false, false, false) => T::StickArrowBottomReverse,
        };
        self.emit(kind, p, p + if dotted { 4 } else { 3 });
        true
    }

    fn title(&mut self) -> bool {
        let p = self.p;
        if !self.ci(p, "title") {
            return false;
        }
        let mut q = p + 5;
        let legacy = self.at(q) == 58;
        if legacy {
            q += 1;
        }
        let next = q + self.len(q);
        if !is_space(self.at(q)) || next >= self.n || !self.has(next, LINE) {
            return false;
        }
        let end = self.line_run(next);
        self.emit(if legacy { T::LegacyTitle } else { T::Title }, p, end);
        true
    }

    fn acc(&mut self) -> bool {
        let p = self.p;
        let descr = self.ci(p, "accdescr");
        if !descr && !self.ci(p, "acctitle") {
            return false;
        }
        let q = self.skip_ws(p + 8);
        let c = self.at(q);
        if c == 58 {
            let end = self.skip_ws(q + 1);
            self.emit(if descr { T::AccDescr } else { T::AccTitle }, p, end);
            self.push(if descr { S::AccDescr } else { S::AccTitle });
            return true;
        }
        if descr && c == 123 {
            self.p = self.skip_ws(q + 1);
            self.push(S::AccDescrMultiline);
            return true;
        }
        false
    }

    fn keywords(&mut self, c: u32) -> bool {
        match c | 32 {
            97 => {
                self.keyword("actor", T::ParticipantActor, Some(S::Id))
                    || self.keyword("alt", T::Alt, Some(S::Line))
                    || self.keyword("and", T::And, Some(S::Line))
                    || self.keyword("activate", T::Activate, Some(S::Id))
                    || self.acc()
                    || self.keyword("autonumber", T::Autonumber, None)
            }
            98 => {
                self.keyword("box", T::Box, Some(S::Line))
                    || self.keyword("break", T::Break, Some(S::Line))
            }
            99 => {
                self.keyword("create", T::Create, None)
                    || self.keyword("critical", T::Critical, Some(S::Line))
            }
            100 => {
                self.keyword("destroy", T::Destroy, Some(S::Id))
                    || self.menu("details", T::Details)
                    || self.keyword("deactivate", T::Deactivate, Some(S::Id))
            }
            101 => {
                self.keyword("else", T::Else, Some(S::Line)) || self.keyword("end", T::End, None)
            }
            108 => {
                self.keyword("loop", T::Loop, Some(S::Line))
                    || self.keyword("left of", T::LeftOf, None)
                    || self.menu("links", T::Links)
                    || self.menu("link", T::Link)
            }
            110 => self.keyword("note", T::Note, None),
            111 => {
                self.keyword("opt", T::Opt, Some(S::Line))
                    || self.keyword("option", T::Option, Some(S::Line))
                    || self.keyword("over", T::Over, None)
                    || self.keyword("off", T::Off, None)
            }
            112 => {
                self.keyword("participant", T::Participant, Some(S::Id))
                    || self.keyword("par", T::Par, Some(S::Line))
                    || self.keyword("par_over", T::ParOver, Some(S::Line))
                    || self.menu("properties", T::Properties)
            }
            114 => {
                self.keyword("rect", T::Rect, Some(S::Line))
                    || self.keyword("right of", T::RightOf, None)
            }
            115 => self.keyword("sequencediagram", T::Sd, None),
            116 => self.title(),
            _ => false,
        }
    }

    fn initial(&mut self) {
        let p = self.p;
        let c = self.at(p);
        if c == 10 {
            let mut q = p + 1;
            while self.at(q) == 10 {
                q += 1;
            }
            self.emit(T::Newline, p, q);
            return;
        }
        if is_space(c) {
            self.p = self.skip_ws(p + self.len(p));
            return;
        }
        if c == 35 || (c == 37 && self.at(p + 1) != 123) {
            self.p = self.eol(p);
            return;
        }
        // Pele looks one UTF-16 unit on, which is half of a character outside the BMP.
        let next = p + self.len(p);
        if c != 125 && c <= 0xffff && self.at(next) == 37 && self.at(next + 1) == 37 {
            self.p = self.eol(p);
            return;
        }
        if (is_digit(c) || c == 46) && self.number() {
            return;
        }
        if c < 128 && (c | 32) >= 97 && (c | 32) <= 122 && self.keywords(c) {
            return;
        }
        if c == 44 {
            self.emit(T::Comma, p, p + 1);
        } else if c == 59 {
            self.emit(T::Newline, p, p + 1);
        } else if self.has(p, ACTOR_START) {
            self.actor();
        } else if c == 45 {
            self.dash();
        } else if c == 60 && self.bytes.get(p..).is_some_and(|r| r.starts_with(b"<<->>")) {
            self.emit(T::BidirectionalSolidArrow, p, p + 5);
        } else if c == 60
            && self
                .bytes
                .get(p..)
                .is_some_and(|r| r.starts_with(b"<<-->>"))
        {
            self.emit(T::BidirectionalDottedArrow, p, p + 6);
        } else if (c == 47 || c == 92) && self.reverse(c) {
        } else if c == 58 {
            let end = self.line_run(p + 1);
            self.emit(T::Txt, p, end);
        } else if c == 43 {
            self.emit(T::Plus, p, p + 1);
        } else if c == 40 && self.at(p + 1) == 41 {
            self.emit(T::Central, p, p + 2);
        } else {
            self.emit(T::Invalid, p, p + 1);
        }
    }

    /// A participant name, up to an alias, a config object, or the end of the statement.
    #[expect(
        clippy::many_single_char_names,
        reason = "mirrors the names of Pele's id"
    )]
    fn id(&mut self) -> bool {
        if self.skip_same_line() {
            return true;
        }
        let p = self.p;
        let c = self.at(p);
        if c == 35 {
            self.p = self.eol(p);
            return true;
        }
        if c == 64 && self.at(p + 1) == 123 {
            self.begin(T::ConfigStart, 2, S::Config);
            return true;
        }
        let mut a = p;
        while a < self.n && self.has(a, ID) {
            a += self.len(a);
        }
        if a > p {
            let w = self.skip_ws(a);
            if self.at(w) == 64 && self.at(w + 1) == 123 {
                self.emit(T::Actor, p, a);
                return true;
            }
            if w > a && self.ci(w, "as") && is_space(self.at(w + 2)) {
                self.emit(T::Actor, p, a);
                self.push(S::Alias);
                return true;
            }
        }
        let mut b = a;
        while b < self.n && self.has(b, ID_WIDE) {
            b += self.len(b);
        }
        if b > p {
            let end = self.at(b);
            // Before any other delimiter the name must be followed by a comment, so it ends at the
            // last one.
            let mut cut = b;
            if b < self.n && end != 10 && end != 59 {
                cut -= 1;
                while cut > p && self.bytes.get(cut) != Some(&b'#') {
                    cut -= 1;
                }
            }
            if cut > p {
                let text = trim(self.slice(p, cut));
                self.emit_text(T::Actor, p, cut, text);
                self.pop();
                return true;
            }
        }
        if self.at(b) == 60 {
            let e = self.eol(p);
            self.emit(T::Invalid, p, e);
            self.pop();
            return true;
        }
        if c != 10 {
            let e = self.eol(p);
            let text = trim(self.slice(p, e));
            self.emit_text(T::Invalid, p, e, text);
            self.pop();
            return true;
        }
        false
    }

    fn step(&mut self) -> bool {
        match self.state {
            S::Initial => {
                self.initial();
                true
            }
            S::Id => self.id(),
            S::Alias => {
                if self.skip_same_line() {
                    return true;
                }
                if self.at(self.p) == 35 {
                    self.p = self.eol(self.p);
                    return true;
                }
                self.pop();
                self.pop();
                if self.word("as") {
                    self.begin(T::As, 2, S::Line);
                } else {
                    self.emit(T::Newline, self.p, self.p);
                }
                true
            }
            S::Line => {
                if self.skip_same_line() {
                    return true;
                }
                if self.at(self.p) == 35 {
                    self.p = self.eol(self.p);
                    return true;
                }
                let end = self.line_run(self.p);
                self.emit(T::RestOfLine, self.p, end);
                self.pop();
                true
            }
            S::Config => {
                let p = self.p;
                if self.at(p) == 125 {
                    let w = self.skip_ws(p + 1);
                    let alias = w > p + 1 && self.ci(w, "as") && is_space(self.at(w + 2));
                    self.emit(T::ConfigEnd, p, p + 1);
                    self.pop();
                    if alias {
                        self.push(S::Alias);
                    } else {
                        self.pop();
                    }
                } else {
                    let q = self.close_brace(p);
                    self.emit(T::ConfigContent, p, q);
                }
                true
            }
            S::AccTitle | S::AccDescr => {
                let kind = if self.state == S::AccTitle {
                    T::AccTitleValue
                } else {
                    T::AccDescrValue
                };
                let e = self.eol(self.p);
                self.emit(kind, self.p, e);
                self.pop();
                true
            }
            S::AccDescrMultiline => {
                let p = self.p;
                if self.at(p) == 125 {
                    self.p += 1;
                    self.pop();
                } else {
                    let q = self.close_brace(p);
                    self.emit(T::AccDescrMultilineValue, p, q);
                }
                true
            }
        }
    }

    /// The offset of the next `}` from `q`, or the end.
    fn close_brace(&self, q: usize) -> usize {
        self.bytes
            .get(q..)
            .and_then(|rest| rest.iter().position(|&b| b == b'}'))
            .map_or(self.n, |at| q + at)
    }
}

/// Splits preprocessed sequence diagram `src` into tokens, ending with [`T::Eof`], or with
/// [`T::Error`] where no rule matches.
pub(crate) fn tokenize(src: &str) -> Vec<Token<'_>> {
    let mut lexer = Lexer {
        bytes: src.as_bytes(),
        n: src.len(),
        next_nl: None,
        p: 0,
        src,
        stack: Vec::new(),
        state: S::Initial,
        tokens: Vec::new(),
    };

    while lexer.p < lexer.n {
        if !lexer.step() {
            let p = lexer.p;
            lexer.tokens.push(Token {
                kind: T::Error,
                start: p,
                text: "",
            });
            return lexer.tokens;
        }
    }

    // Mermaid's lexer tries its rules once more on the empty remainder before reporting the end.
    let n = lexer.n;
    let last = match lexer.state {
        S::Initial | S::Alias => Some(T::Newline),
        S::Line => Some(T::RestOfLine),
        S::AccTitle => Some(T::AccTitleValue),
        S::AccDescr => Some(T::AccDescrValue),
        S::AccDescrMultiline => Some(T::AccDescrMultilineValue),
        S::Id | S::Config => None,
    };
    if let Some(kind) = last {
        lexer.emit(kind, n, n);
    }
    lexer.tokens.push(Token {
        kind: T::Eof,
        start: n,
        text: "",
    });
    lexer.tokens
}
