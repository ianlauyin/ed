//! The flowchart lexer: a state machine equivalent to Mermaid's Jison lexer. Port of Pele's
//! `src/diagrams/flowchart/lexer.ts`.
//!
//! Pele reads UTF-16 code units. This port reads UTF-8 bytes: every delimiter is ASCII, and
//! non-ASCII characters are decoded whole wherever a test (`\s`, `\p{L}`) looks at them, so
//! token boundaries match. Each regular expression in Pele is a hand-written matcher below.

use std::borrow::Cow;

use super::tokens::T;
use crate::util::chars::{is_space, is_word};
use crate::util::unicode::is_unicode_letter;

/// A token: its kind, its start as a UTF-8 offset, and its text.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Token<'a> {
    /// The token kind.
    pub(crate) kind: T,
    /// The UTF-8 offset of the token in the source.
    pub(crate) start: usize,
    /// The matched text. Shape data strings own a rewritten copy.
    pub(crate) text: Cow<'a, str>,
}

/// Lexer start conditions, as in Mermaid's grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum S {
    AccDescr,
    AccDescrMultiline,
    AccTitle,
    CallbackArgs,
    CallbackName,
    Click,
    Dir,
    DottedEdgeText,
    EdgeText,
    EllipseText,
    Initial,
    MdString,
    ShapeData,
    ShapeDataStr,
    String,
    Text,
    ThickEdgeText,
    TrapText,
}

/// The code past the end of the source or inside a multi-byte character, which no test matches.
/// Pele's `charCodeAt` returns `NaN` there.
const NONE: u32 = u32::MAX;

/// The absent second link character of an edge text state. It never equals a real code.
const NO_CHAR: u32 = u32::MAX - 1;

/// Characters of a node string, as Pele's `NODE_CHAR` table marks them.
const NODE_CHAR: [bool; 128] = {
    let mut table = [false; 128];
    let chars = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!\"#$%&'*+.`?\\_/";
    let mut i = 0;
    while i < chars.len() {
        table[chars[i] as usize] = true;
        i += 1;
    }
    table
};

/// The arrow families an edge text state ends with.
#[derive(Clone, Copy)]
enum Edge {
    Dotted,
    Normal,
    Thick,
}

/// Lexer state. Pele keeps these as closure variables of `tokenize`.
struct Lexer<'a> {
    at_next: Option<usize>,
    bytes: &'a [u8],
    dir_failed: usize,
    dir_next: Option<usize>,
    first_graph: bool,
    line_end: Option<usize>,
    p: usize,
    run_at: Option<usize>,
    run_end: usize,
    src: &'a str,
    stack: Vec<S>,
    state: S,
    tokens: Vec<Token<'a>>,
    ws: Option<(usize, usize)>,
}

/// Splits `src` into tokens. The list ends with an `End` token, or with an `Error` token where
/// no rule matches.
pub(crate) fn tokenize(src: &str) -> Vec<Token<'_>> {
    let mut lexer = Lexer {
        at_next: src.find('@'),
        bytes: src.as_bytes(),
        dir_failed: 0,
        dir_next: src.find("direction"),
        first_graph: true,
        line_end: None,
        p: 0,
        run_at: None,
        run_end: 0,
        src,
        stack: Vec::new(),
        state: S::Initial,
        tokens: Vec::new(),
        ws: None,
    };
    lexer.run();
    lexer.tokens
}

impl Lexer<'_> {
    /// The byte at `i`, or [`NONE`].
    fn byte(&self, i: usize) -> u32 {
        self.bytes.get(i).map_or(NONE, |&b| u32::from(b))
    }

    /// The character at `i` when `i` starts one, or [`NONE`].
    fn code(&self, i: usize) -> u32 {
        match self.bytes.get(i) {
            None => NONE,
            Some(&b) if b < 0x80 => u32::from(b),
            Some(_) => self
                .src
                .get(i..)
                .and_then(|rest| rest.chars().next())
                .map_or(NONE, u32::from),
        }
    }

    /// The offset after the character at `i`.
    fn next(&self, i: usize) -> usize {
        i + self
            .src
            .get(i..)
            .and_then(|rest| rest.chars().next())
            .map_or(1, char::len_utf8)
    }

    /// The end of a run of `\s` starting at `i`.
    fn skip_space(&self, mut i: usize) -> usize {
        while is_space(self.code(i)) {
            i = self.next(i);
        }
        i
    }

    /// The first offset of `needle` at or after `from`.
    fn find(&self, needle: &str, from: usize) -> Option<usize> {
        self.src
            .get(from..)
            .and_then(|rest| rest.find(needle))
            .map(|at| from + at)
    }

    /// The first offset of byte `needle` at or after `from`, or the end of the source.
    fn find_byte(&self, needle: u8, from: usize) -> usize {
        self.bytes
            .get(from..)
            .and_then(|rest| rest.iter().position(|&b| b == needle))
            .map_or(self.bytes.len(), |at| from + at)
    }

    fn starts_with(&self, word: &str, at: usize) -> bool {
        self.bytes
            .get(at..)
            .is_some_and(|rest| rest.starts_with(word.as_bytes()))
    }

    fn emit(&mut self, kind: T, start: usize, end: usize) {
        self.tokens.push(Token {
            kind,
            start,
            text: Cow::Borrowed(self.src.get(start..end).unwrap_or("")),
        });
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

    /// A keyword at the cursor that no word character follows.
    fn kw(&self, word: &str) -> bool {
        self.starts_with(word, self.p) && !is_word(self.byte(self.p + word.len()))
    }

    /// The end of the run of whitespace at `q`, memoized like Pele's `wsFrom` and `wsTo`.
    fn space_run(&mut self, q: usize) -> usize {
        if let Some((from, to)) = self.ws
            && q >= from
            && q <= to
        {
            return to;
        }

        if is_space(self.code(q)) {
            let end = self.skip_space(q);
            self.ws = Some((q, end));
            return end;
        }

        q
    }

    /// First character after any run of whitespace starting at `q`, skipping one optional x, o
    /// or <.
    fn link_char(&mut self, q: usize) -> u32 {
        let j = self.space_run(q);
        let d = self.byte(j);
        if d == u32::from(b'x') || d == u32::from(b'o') || d == u32::from(b'<') {
            return self.byte(j + 1);
        }
        d
    }

    fn invis_ahead(&mut self, q: usize) -> bool {
        let j = self.space_run(q);
        self.byte(j) == u32::from(b'~')
    }

    /// Skips an optional `[xo<]` at `i`.
    fn arrow_tail(&self, i: usize) -> usize {
        match self.bytes.get(i) {
            Some(b'x' | b'o' | b'<') => i + 1,
            _ => i,
        }
    }

    /// `/\s*[xo<]?CC+[Cxo>]\s*/y` for line character `C` (`RE_LINK`, `RE_THICK_LINK`).
    fn re_line_link(&self, q: usize, line: u8) -> Option<usize> {
        let start = self.arrow_tail(self.skip_space(q));
        let mut i = start;
        while self.bytes.get(i) == Some(&line) {
            i += 1;
        }

        let count = i - start;
        if count >= 2 && matches!(self.bytes.get(i), Some(b'x' | b'o' | b'>')) {
            i += 1;
        } else if count < 3 {
            return None;
        }
        Some(self.skip_space(i))
    }

    /// `/\s*[xo<]?CC\s*/y` for line character `C` (`RE_START_LINK`, `RE_THICK_START`).
    fn re_line_start(&self, q: usize, line: u8) -> Option<usize> {
        let i = self.arrow_tail(self.skip_space(q));
        if self.bytes.get(i) == Some(&line) && self.bytes.get(i + 1) == Some(&line) {
            return Some(self.skip_space(i + 2));
        }
        None
    }

    /// `/\s*[xo<]?-?\.+-[xo>]?\s*/y` (`RE_DOTTED_LINK`).
    fn re_dotted_link(&self, q: usize) -> Option<usize> {
        let mut i = self.arrow_tail(self.skip_space(q));
        if self.bytes.get(i) == Some(&b'-') {
            i += 1;
        }

        let dots = i;
        while self.bytes.get(i) == Some(&b'.') {
            i += 1;
        }
        if i == dots || self.bytes.get(i) != Some(&b'-') {
            return None;
        }

        i += 1;
        if matches!(self.bytes.get(i), Some(b'x' | b'o' | b'>')) {
            i += 1;
        }
        Some(self.skip_space(i))
    }

    /// `/\s*[xo<]?-\.\s*/y` (`RE_DOTTED_START`).
    fn re_dotted_start(&self, q: usize) -> Option<usize> {
        let i = self.arrow_tail(self.skip_space(q));
        if self.bytes.get(i) == Some(&b'-') && self.bytes.get(i + 1) == Some(&b'.') {
            return Some(self.skip_space(i + 2));
        }
        None
    }

    /// `/\s*~~~+\s*/y` (`RE_INVIS_LINK`).
    fn re_invis_link(&self, q: usize) -> Option<usize> {
        let start = self.skip_space(q);
        let mut i = start;
        while self.bytes.get(i) == Some(&b'~') {
            i += 1;
        }
        if i - start < 3 {
            return None;
        }
        Some(self.skip_space(i))
    }

    /// The closing arrow matcher of an edge text state.
    fn re_edge(&self, edge: Edge, q: usize) -> Option<usize> {
        match edge {
            Edge::Dotted => self.re_dotted_link(q),
            Edge::Normal => self.re_line_link(q, b'-'),
            Edge::Thick => self.re_line_link(q, b'='),
        }
    }

    /// `/NAME\s*C\s*/y` (`RE_ACC_TITLE`, `RE_ACC_DESCR`, `RE_ACC_DESCR_ML`).
    fn re_acc(&self, name: &str, punct: u8) -> Option<usize> {
        if !self.starts_with(name, self.p) {
            return None;
        }
        let i = self.skip_space(self.p + name.len());
        if self.bytes.get(i) != Some(&punct) {
            return None;
        }
        Some(self.skip_space(i + 1))
    }

    /// `/WORD\s+/y` (`RE_CALL`, `RE_CLICK`).
    fn re_word_space(&self, word: &str) -> Option<usize> {
        if !self.starts_with(word, self.p) {
            return None;
        }
        let start = self.p + word.len();
        let end = self.skip_space(start);
        (end > start).then_some(end)
    }

    /// `/(?:\r?\n)*\s*\n/y` (`RE_NODIR`): the whitespace run at `q`, up to its last newline.
    fn re_nodir(&self, q: usize) -> Option<usize> {
        let end = self.skip_space(q);
        self.bytes
            .get(q..end)
            .and_then(|run| run.iter().rposition(|&b| b == b'\n'))
            .map(|at| q + at + 1)
    }

    /// `/\s*(?:LR|RL|TB|BT|TD|BR)\b/y` (`RE_DIR`).
    fn re_dir(&self, q: usize) -> Option<usize> {
        let i = self.skip_space(q);
        let pair = self.bytes.get(i..i + 2)?;
        let known = matches!(pair, b"LR" | b"RL" | b"TB" | b"BT" | b"TD" | b"BR");
        (known && !is_word(self.byte(i + 2))).then_some(i + 2)
    }

    /// `/\s*[<>^]/y` (`RE_DIR_SYM`).
    fn re_dir_sym(&self, q: usize) -> Option<usize> {
        let i = self.skip_space(q);
        matches!(self.bytes.get(i), Some(b'<' | b'>' | b'^')).then_some(i + 1)
    }

    /// `/\s*v\b/y` (`RE_DIR_V`).
    fn re_dir_v(&self, q: usize) -> Option<usize> {
        let i = self.skip_space(q);
        (self.bytes.get(i) == Some(&b'v') && !is_word(self.byte(i + 1))).then_some(i + 1)
    }

    /// The first line terminator (`/[\n\r  ]/g`) at or after `from`, or the end.
    fn line_end_from(&self, from: usize) -> usize {
        let mut i = from;
        while let Some(&b) = self.bytes.get(i) {
            if b == b'\n' || b == b'\r' {
                return i;
            }
            if b == 0xe2
                && self.bytes.get(i + 1) == Some(&0x80)
                && matches!(self.bytes.get(i + 2), Some(0xa8 | 0xa9))
            {
                return i;
            }
            i += 1;
        }
        self.bytes.len()
    }

    /// `/.*direction\s+XX[^\n]*/y` (`RE_DIRECTION`). The greedy `.*` picks the last
    /// `direction` before the first line terminator, and the match runs to the next `\n`.
    fn re_direction(&self, q: usize, dir: [u8; 2]) -> Option<usize> {
        let limit = self.line_end_from(q);
        let line = self.src.get(q..limit)?;
        let mut found = None;
        for (at, _) in line.match_indices("direction") {
            let start = q + at + "direction".len();
            let i = self.skip_space(start);
            if i > start && self.bytes.get(i..i + 2) == Some(&dir[..]) {
                found = Some(i + 2);
            }
        }
        found.map(|end| self.find_byte(b'\n', end))
    }

    /// `/(?:\r?\n)+/y` (`RE_NEWLINE`).
    fn re_newline(&self, q: usize) -> Option<usize> {
        let mut i = q;
        loop {
            match self.bytes.get(i) {
                Some(b'\n') => i += 1,
                Some(b'\r') if self.bytes.get(i + 1) == Some(&b'\n') => i += 2,
                _ => break,
            }
        }
        (i > q).then_some(i)
    }

    /// `/[^\S\n\r]+/y` (`RE_SPACE`).
    fn re_space(&self, q: usize) -> Option<usize> {
        let mut i = q;
        loop {
            let c = self.code(i);
            if !is_space(c) || c == 10 || c == 13 {
                break;
            }
            i = self.next(i);
        }
        (i > q).then_some(i)
    }

    /// `/\(\s*\)/y` (`RE_EMPTY_CALL`).
    fn re_empty_call(&self, q: usize) -> Option<usize> {
        if self.bytes.get(q) != Some(&b'(') {
            return None;
        }
        let i = self.skip_space(q + 1);
        (self.bytes.get(i) == Some(&b')')).then_some(i + 1)
    }

    fn quote(&mut self) {
        if self.byte(self.p + 1) == u32::from(b'`') {
            self.push(S::MdString);
            self.p += 2;
        } else {
            self.push(S::String);
            self.p += 1;
        }
    }

    fn invis_link(&mut self) -> bool {
        if !self.invis_ahead(self.p) {
            return false;
        }
        let Some(e) = self.re_invis_link(self.p) else {
            return false;
        };
        self.emit(T::Link, self.p, e);
        true
    }

    fn opener(&mut self, c: u32) -> bool {
        let p = self.p;
        let d = self.byte(p + 1);
        match u8::try_from(c) {
            Ok(b'(') => {
                if d == u32::from(b'-') {
                    self.push(S::EllipseText);
                    self.emit(T::EllipseStart, p, p + 2);
                } else if d == u32::from(b'[') {
                    self.push(S::Text);
                    self.emit(T::StadiumStart, p, p + 2);
                } else if d == u32::from(b'(') && self.byte(p + 2) == u32::from(b'(') {
                    self.push(S::Text);
                    self.emit(T::DoubleCircleStart, p, p + 3);
                } else {
                    self.push(S::Text);
                    self.emit(T::Ps, p, p + 1);
                }
                true
            }
            Ok(b'[') => {
                if d == u32::from(b'[') {
                    self.push(S::Text);
                    self.emit(T::SubroutineStart, p, p + 2);
                } else if d == u32::from(b'(') {
                    self.push(S::Text);
                    self.emit(T::CylinderStart, p, p + 2);
                } else if d == u32::from(b'/') {
                    self.push(S::TrapText);
                    self.emit(T::TrapStart, p, p + 2);
                } else if d == u32::from(b'\\') {
                    self.push(S::TrapText);
                    self.emit(T::InvTrapStart, p, p + 2);
                } else {
                    self.push(S::Text);
                    self.emit(T::Sqs, p, p + 1);
                }
                true
            }
            Ok(b'|') => {
                self.push(S::Text);
                self.emit(T::Pipe, p, p + 1);
                true
            }
            Ok(b'{') => {
                self.push(S::Text);
                self.emit(T::DiamondStart, p, p + 1);
                true
            }
            _ => false,
        }
    }

    fn graph(&mut self, len: usize) {
        if self.first_graph {
            self.first_graph = false;
            self.push(S::Dir);
        }
        self.emit(T::Graph, self.p, self.p + len);
    }

    fn node_string(&mut self) -> bool {
        let n = self.bytes.len();
        let mut q = self.p;
        while let Some(&ch) = self.bytes.get(q) {
            if ch < 128 && NODE_CHAR[usize::from(ch)] {
                q += 1;
            } else if ch == b'-' && q + 1 < n {
                if matches!(self.bytes.get(q + 1), Some(b'>' | b'-' | b'.')) {
                    break;
                }
                q += 1;
            } else {
                break;
            }
        }

        if q == self.p {
            return false;
        }
        self.emit(T::NodeString, self.p, q);
        true
    }

    fn link(&mut self, c: u32) -> bool {
        let p = self.p;
        let d = self.link_char(p);
        if d == u32::from(b'-') {
            if let Some(e) = self.re_line_link(p, b'-') {
                self.emit(T::Link, p, e);
                return true;
            }
            if let Some(e) = self.re_line_start(p, b'-') {
                self.push(S::EdgeText);
                self.emit(T::StartLink, p, e);
                return true;
            }
            if let Some(e) = self.re_dotted_link(p) {
                self.emit(T::Link, p, e);
                return true;
            }
            if let Some(e) = self.re_dotted_start(p) {
                self.push(S::DottedEdgeText);
                self.emit(T::StartLink, p, e);
                return true;
            }
        } else if d == u32::from(b'=') {
            if let Some(e) = self.re_line_link(p, b'=') {
                self.emit(T::Link, p, e);
                return true;
            }
            if let Some(e) = self.re_line_start(p, b'=') {
                self.push(S::ThickEdgeText);
                self.emit(T::StartLink, p, e);
                return true;
            }
        } else if d == u32::from(b'.')
            && let Some(e) = self.re_dotted_link(p)
        {
            self.emit(T::Link, p, e);
            return true;
        }

        c != u32::from(b'-') && c != u32::from(b'=') && c != u32::from(b'.') && self.invis_link()
    }

    fn keyword(&mut self, kind: T, len: usize) -> bool {
        self.emit(kind, self.p, self.p + len);
        true
    }

    fn initial(&mut self, c: u32) -> bool {
        let p = self.p;
        match u8::try_from(c) {
            Ok(b'a') => {
                if self.starts_with("acc", p) {
                    if let Some(e) = self.re_acc("accTitle", b':') {
                        self.push(S::AccTitle);
                        self.emit(T::AccTitle, p, e);
                        return true;
                    }
                    if let Some(e) = self.re_acc("accDescr", b':') {
                        self.push(S::AccDescr);
                        self.emit(T::AccDescr, p, e);
                        return true;
                    }
                    if let Some(e) = self.re_acc("accDescr", b'{') {
                        self.push(S::AccDescrMultiline);
                        self.p = e;
                        return true;
                    }
                }
            }
            Ok(b'@') => {
                if self.byte(p + 1) == u32::from(b'{') {
                    self.push(S::ShapeData);
                    self.tokens.push(Token {
                        kind: T::ShapeData,
                        start: p,
                        text: Cow::Borrowed(""),
                    });
                    self.p += 2;
                    return true;
                }
            }
            Ok(b'c') => {
                if let Some(e) = self.re_word_space("call") {
                    self.push(S::CallbackName);
                    self.p = e;
                    return true;
                }
                if self.kw("classDef") {
                    return self.keyword(T::ClassDef, 8);
                }
                if self.kw("class") {
                    return self.keyword(T::Class, 5);
                }
                if let Some(e) = self.re_word_space("click") {
                    self.push(S::Click);
                    self.p = e;
                    return true;
                }
            }
            Ok(b'"') => {
                self.quote();
                return true;
            }
            Ok(b's') => {
                if self.kw("style") {
                    return self.keyword(T::Style, 5);
                }
                if self.kw("swimlane-beta") {
                    self.graph(13);
                    return true;
                }
                if self.kw("subgraph") {
                    return self.keyword(T::Subgraph, 8);
                }
            }
            Ok(b'd') => {
                if self.kw("default") {
                    return self.keyword(T::Default, 7);
                }
            }
            Ok(b'l') => {
                if self.kw("linkStyle") {
                    return self.keyword(T::LinkStyle, 9);
                }
            }
            Ok(b'i') => {
                if self.kw("interpolate") {
                    return self.keyword(T::Interpolate, 11);
                }
            }
            Ok(b'h') => {
                // The token includes the whitespace character, which may be more than one byte.
                if self.starts_with("href", p) && is_space(self.code(p + 4)) {
                    let end = self.next(p + 4);
                    self.emit(T::Href, p, end);
                    return true;
                }
            }
            Ok(b'f') => {
                if self.kw("flowchart-elk") {
                    self.graph(13);
                    return true;
                }
                if self.kw("flowchart") {
                    self.graph(9);
                    return true;
                }
            }
            Ok(b'g') => {
                if self.kw("graph") {
                    self.graph(5);
                    return true;
                }
            }
            Ok(b'e') => {
                if self.kw("end") {
                    let e = self.skip_space(p + 3);
                    self.emit(T::SubgraphEnd, p, e);
                    return true;
                }
            }
            Ok(b'_') => {
                for target in ["_self", "_blank", "_parent", "_top"] {
                    if self.kw(target) {
                        return self.keyword(T::LinkTarget, target.len());
                    }
                }
            }
            _ => {}
        }

        if self.direction() {
            return true;
        }

        if self.link_id(c) {
            return true;
        }

        if (48..=57).contains(&c) {
            let mut q = p + 1;
            while self.bytes.get(q).is_some_and(u8::is_ascii_digit) {
                q += 1;
            }
            self.emit(T::Num, p, q);
            return true;
        }

        match u8::try_from(c) {
            Ok(b'#') => return self.keyword(T::Brkt, 1),
            Ok(b':') => {
                if self.byte(p + 1) == u32::from(b':') && self.byte(p + 2) == u32::from(b':') {
                    return self.keyword(T::StyleSeparator, 3);
                }
                return self.keyword(T::Colon, 1);
            }
            Ok(b'&') => return self.keyword(T::Amp, 1),
            Ok(b';') => return self.keyword(T::Semi, 1),
            Ok(b',') => return self.keyword(T::Comma, 1),
            Ok(b'*') => return self.keyword(T::Mult, 1),
            Ok(b'x' | b'o' | b'.') => return self.link(c) || self.node_string(),
            Ok(b'<') => {
                if self.link(c) {
                    return true;
                }
                return self.keyword(T::TagStart, 1);
            }
            Ok(b'-') => {
                if self.link(c) || self.node_string() {
                    return true;
                }
                return self.keyword(T::Minus, 1);
            }
            Ok(b'=') => return self.link(c),
            Ok(b'~') => return self.invis_link(),
            Ok(b'(' | b'|' | b'{') => return self.opener(c),
            Ok(b'[') => {
                if self.byte(p + 1) == u32::from(b'|') {
                    return self.keyword(T::VertexWithPropsStart, 2);
                }
                return self.opener(c);
            }
            Ok(b'>') => {
                self.push(S::Text);
                return self.keyword(T::TagEnd, 1);
            }
            Ok(b'^') => return self.keyword(T::Up, 1),
            Ok(b'\\') => {
                if self.byte(p + 1) == u32::from(b'|') {
                    return self.keyword(T::Sep, 2);
                }
                return self.node_string();
            }
            Ok(b'v') => {
                if !is_word(self.byte(p + 1)) {
                    return self.keyword(T::Down, 1);
                }
                return self.node_string();
            }
            _ => {}
        }

        if is_space(c) {
            if self.link(c) {
                return true;
            }
            if let Some(e) = self.re_newline(p) {
                self.emit(T::Newline, p, e);
                return true;
            }
            if let Some(e) = self.re_space(p) {
                self.emit(T::Space, p, e);
                return true;
            }
            return false;
        }

        if c < 128 {
            return self.node_string();
        }
        if is_unicode_letter(c) {
            let end = self.next(p);
            self.emit(T::UnicodeText, p, end);
            return true;
        }
        false
    }

    /// A `direction XX` statement anywhere on the current line.
    fn direction(&mut self) -> bool {
        let p = self.p;
        let Some(mut next) = self.dir_next else {
            return false;
        };
        if p < self.dir_failed {
            return false;
        }

        if next < p {
            self.dir_next = self.find("direction", p);
            let Some(found) = self.dir_next else {
                return false;
            };
            next = found;
        }

        let line_end = match self.line_end {
            Some(end) if end >= p => end,
            _ => {
                let end = self.line_end_from(p);
                self.line_end = Some(end);
                end
            }
        };
        if next >= line_end {
            return false;
        }

        let kinds = [
            (T::DirectionTb, *b"TB"),
            (T::DirectionBt, *b"BT"),
            (T::DirectionRl, *b"RL"),
            (T::DirectionLr, *b"LR"),
            (T::DirectionTd, *b"TD"),
        ];
        for (kind, dir) in kinds {
            if let Some(e) = self.re_direction(p, dir) {
                self.emit(kind, p, e);
                return true;
            }
        }

        // No direction statement starts anywhere in the rest of this line either.
        self.dir_failed = line_end;
        false
    }

    /// A link id is a run of non-space characters up to its last `@` that is not followed by
    /// `{` or `"`.
    fn link_id(&mut self, c: u32) -> bool {
        if self.at_next.is_none() || c == u32::from(b'"') || is_space(c) {
            return false;
        }

        let p = self.p;
        let n = self.bytes.len();
        if p >= self.run_end {
            if self.at_next.is_some_and(|at| at < p) {
                self.at_next = self.find("@", p);
            }

            let mut q = p;
            while q < n {
                let ch = self.code(q);
                if ch == u32::from(b'"') || is_space(ch) {
                    break;
                }
                q += 1;
            }

            self.run_end = q;
            self.run_at = None;
            if let Some(at) = self.at_next
                && at < q
            {
                // Pele starts at `min(q, n - 1) - 1`, so a run that ends the source never
                // considers its last unit.
                let top = q.min(n - 1);
                for k in (at..top).rev() {
                    if self.bytes.get(k) != Some(&b'@') {
                        continue;
                    }
                    let after = self.byte(k + 1);
                    if after != u32::from(b'{') && after != u32::from(b'"') {
                        self.run_at = Some(k);
                        break;
                    }
                }
            }
        }

        if let Some(at) = self.run_at
            && at > p
        {
            self.emit(T::LinkId, p, at + 1);
            return true;
        }
        false
    }

    #[expect(clippy::many_single_char_names, reason = "keeps Pele's names")]
    fn edge_text(&mut self, c: u32, edge: Edge, a: u8, b: u32) -> bool {
        if c == u32::from(b'"') {
            self.quote();
            return true;
        }

        let p = self.p;
        let a32 = u32::from(a);
        let d = self.link_char(p);
        if (d == a32 || d == b)
            && let Some(e) = self.re_edge(edge, p)
        {
            self.pop();
            self.emit(T::Link, p, e);
            return true;
        }

        if c == a32 && (a != b'-' || self.byte(p + 1) == u32::from(b'-')) {
            return false;
        }

        let n = self.bytes.len();
        let mut q = p + 1;
        while q < n {
            let ch = self.code(q);
            if ch == u32::from(b'"') {
                break;
            }
            if ch == a32 {
                if a != b'-' || self.byte(q + 1) == u32::from(b'-') {
                    break;
                }
            } else if ch == u32::from(b'x')
                || ch == u32::from(b'o')
                || ch == u32::from(b'<')
                || ch == b
                || is_space(ch)
            {
                let dd = self.link_char(q);
                if (dd == a32 || dd == b) && self.re_edge(edge, q).is_some() {
                    break;
                }
            }
            q += 1;
        }
        self.emit(T::EdgeText, p, q);
        true
    }

    fn text_state(&mut self, c: u32) -> bool {
        let p = self.p;
        let d = self.byte(p + 1);
        match u8::try_from(c) {
            Ok(b'"') => self.quote(),
            Ok(b'(' | b'[' | b'{') => {
                self.opener(c);
            }
            Ok(b']') => {
                self.pop();
                if d == u32::from(b')') {
                    self.emit(T::StadiumEnd, p, p + 2);
                } else if d == u32::from(b']') {
                    self.emit(T::SubroutineEnd, p, p + 2);
                } else {
                    self.emit(T::Sqe, p, p + 1);
                }
            }
            Ok(b')') => {
                self.pop();
                if d == u32::from(b']') {
                    self.emit(T::CylinderEnd, p, p + 2);
                } else if d == u32::from(b')') && self.byte(p + 2) == u32::from(b')') {
                    self.emit(T::DoubleCircleEnd, p, p + 3);
                } else {
                    self.emit(T::Pe, p, p + 1);
                }
            }
            Ok(b'|') => {
                self.pop();
                self.emit(T::Pipe, p, p + 1);
            }
            Ok(b'}') => {
                self.pop();
                self.emit(T::DiamondStop, p, p + 1);
            }
            _ => {
                if (c == u32::from(b'~') || is_space(c)) && self.invis_link() {
                    return true;
                }
                let mut q = p + 1;
                while let Some(&ch) = self.bytes.get(q) {
                    if matches!(ch, b'[' | b']' | b'(' | b')' | b'{' | b'}' | b'|' | b'"') {
                        break;
                    }
                    q += 1;
                }
                self.emit(T::Text, p, q);
            }
        }
        true
    }

    #[expect(clippy::many_single_char_names, reason = "keeps Pele's names")]
    fn ellipse_text(&mut self, c: u32) -> bool {
        let p = self.p;
        let d = self.byte(p + 1);
        if c == u32::from(b'"') {
            self.quote();
        } else if (c == u32::from(b'~') || is_space(c)) && self.invis_link() {
            return true;
        } else if matches!(u8::try_from(c), Ok(b'-' | b'/' | b')')) && d == u32::from(b')') {
            self.pop();
            self.emit(T::EllipseEnd, p, p + 2);
        } else if matches!(u8::try_from(c), Ok(b'(' | b'[' | b'{')) {
            self.opener(c);
        } else if matches!(u8::try_from(c), Ok(b')' | b']' | b'}')) {
            return false;
        } else {
            let n = self.bytes.len();
            let mut q = p + 1;
            while q < n {
                let ch = self.code(q);
                if matches!(
                    u8::try_from(ch),
                    Ok(b'"' | b'(' | b')' | b'[' | b']' | b'{' | b'}')
                ) {
                    break;
                }
                if matches!(u8::try_from(ch), Ok(b'-' | b'/'))
                    && self.byte(q + 1) == u32::from(b')')
                {
                    break;
                }
                if (ch == u32::from(b'~') || is_space(ch))
                    && self.invis_ahead(q)
                    && self.re_invis_link(q).is_some()
                {
                    break;
                }
                q += 1;
            }
            self.emit(T::Text, p, q);
        }
        true
    }

    fn trap_text(&mut self, c: u32) -> bool {
        let p = self.p;
        let d = self.byte(p + 1);
        if c == u32::from(b'"') {
            self.quote();
            return true;
        }
        if (c == u32::from(b'~') || is_space(c)) && self.invis_link() {
            return true;
        }

        let close = u32::from(b']');
        match u8::try_from(c) {
            Ok(b'(') => {
                if d == u32::from(b'-')
                    || d == u32::from(b'[')
                    || (d == u32::from(b'(') && self.byte(p + 2) == u32::from(b'('))
                {
                    self.opener(c);
                } else if d == close {
                    self.pop();
                    self.emit(T::TrapEnd, p, p + 2);
                } else {
                    self.opener(c);
                }
            }
            Ok(b'[' | b'{') => {
                self.opener(c);
            }
            Ok(b'\\') => {
                if d == close {
                    self.pop();
                    self.emit(T::TrapEnd, p, p + 2);
                } else {
                    self.emit(T::Text, p, p + 1);
                }
            }
            Ok(b'/') => {
                if d == close {
                    self.pop();
                    self.emit(T::InvTrapEnd, p, p + 2);
                } else {
                    self.emit(T::Text, p, p + 1);
                }
            }
            Ok(b']' | b')') => {
                if d != close {
                    return false;
                }
                self.pop();
                self.emit(T::TrapEnd, p, p + 2);
            }
            Ok(b'}') => return false,
            _ => {
                if (c == u32::from(b'?') || c == u32::from(b'=')) && d == close {
                    self.pop();
                    self.emit(T::TrapEnd, p, p + 2);
                    return true;
                }
                let mut q = p + 1;
                while let Some(&ch) = self.bytes.get(q) {
                    if matches!(ch, b'\\' | b'[' | b']' | b'(' | b')' | b'{' | b'}' | b'/') {
                        break;
                    }
                    q += 1;
                }
                self.emit(T::Text, p, q);
            }
        }
        true
    }

    fn run(&mut self) {
        let n = self.bytes.len();
        loop {
            if self.p >= n {
                let kind = match self.state {
                    S::Initial => Some(T::Eof),
                    S::AccTitle => Some(T::AccTitleValue),
                    S::AccDescr => Some(T::AccDescrValue),
                    S::AccDescrMultiline => Some(T::AccDescrMultilineValue),
                    S::Click => Some(T::Click),
                    S::CallbackName => Some(T::CallbackName),
                    S::CallbackArgs => Some(T::CallbackArgs),
                    _ => None,
                };
                if let Some(kind) = kind {
                    self.emit(kind, n, n);
                }
                break;
            }

            let p = self.p;
            let c = self.code(p);
            let ok = match self.state {
                S::Initial => self.initial(c),
                S::String => {
                    if c == u32::from(b'"') {
                        if self.byte(p + 1) == u32::from(b'`') {
                            self.push(S::MdString);
                            self.p += 2;
                        } else {
                            self.pop();
                            self.p += 1;
                        }
                    } else {
                        let e = self.find_byte(b'"', p);
                        self.emit(T::Str, p, e);
                    }
                    true
                }
                S::MdString => {
                    if c == u32::from(b'`') {
                        if self.byte(p + 1) == u32::from(b'"') {
                            self.pop();
                            self.p += 2;
                            true
                        } else {
                            false
                        }
                    } else if c == u32::from(b'"') {
                        self.quote();
                        true
                    } else {
                        let mut q = p + 1;
                        while self
                            .bytes
                            .get(q)
                            .is_some_and(|&ch| ch != b'`' && ch != b'"')
                        {
                            q += 1;
                        }
                        self.emit(T::MdStr, p, q);
                        true
                    }
                }
                S::AccTitle | S::AccDescr => {
                    let e = self.find_byte(b'\n', p);
                    let kind = if self.state == S::AccTitle {
                        T::AccTitleValue
                    } else {
                        T::AccDescrValue
                    };
                    self.pop();
                    self.emit(kind, p, e);
                    true
                }
                S::AccDescrMultiline => {
                    if c == u32::from(b'}') {
                        self.pop();
                        self.p += 1;
                    } else {
                        let e = self.find_byte(b'}', p);
                        self.emit(T::AccDescrMultilineValue, p, e);
                    }
                    true
                }
                S::Dir => {
                    if c == u32::from(b'"') {
                        self.quote();
                        true
                    } else if let Some(e) = self.re_nodir(p) {
                        self.pop();
                        self.emit(T::NoDir, p, e);
                        true
                    } else if let Some(e) = self
                        .re_dir(p)
                        .or_else(|| self.re_dir_sym(p))
                        .or_else(|| self.re_dir_v(p))
                    {
                        self.pop();
                        self.emit(T::Dir, p, e);
                        true
                    } else {
                        self.invis_link() || self.opener(c)
                    }
                }
                S::Text => self.text_state(c),
                S::EllipseText => self.ellipse_text(c),
                S::TrapText => self.trap_text(c),
                S::EdgeText => self.edge_text(c, Edge::Normal, b'-', NO_CHAR),
                S::ThickEdgeText => self.edge_text(c, Edge::Thick, b'=', NO_CHAR),
                S::DottedEdgeText => self.edge_text(c, Edge::Dotted, b'.', u32::from(b'-')),
                S::Click => {
                    if c == u32::from(b'"') {
                        self.quote();
                    } else if is_space(c) {
                        self.pop();
                        self.p = self.next(p);
                    } else {
                        let mut q = p + 1;
                        while q < n && !is_space(self.code(q)) {
                            q += 1;
                        }
                        self.emit(T::Click, p, q);
                    }
                    true
                }
                S::CallbackName => {
                    if c == u32::from(b'(') {
                        let e = self.re_empty_call(p);
                        self.pop();
                        if let Some(e) = e {
                            self.p = e;
                        } else {
                            self.push(S::CallbackArgs);
                            self.p += 1;
                        }
                    } else {
                        let e = self.find_byte(b'(', p);
                        self.emit(T::CallbackName, p, e);
                    }
                    true
                }
                S::CallbackArgs => {
                    if c == u32::from(b')') {
                        self.pop();
                        self.p += 1;
                    } else {
                        let e = self.find_byte(b')', p);
                        self.emit(T::CallbackArgs, p, e);
                    }
                    true
                }
                S::ShapeData => {
                    if c == u32::from(b'"') {
                        self.push(S::ShapeDataStr);
                        self.emit(T::ShapeData, p, p + 1);
                        true
                    } else if c == u32::from(b'}') {
                        self.pop();
                        self.p += 1;
                        true
                    } else if c == u32::from(b'^') {
                        false
                    } else {
                        let mut q = p + 1;
                        while self
                            .bytes
                            .get(q)
                            .is_some_and(|&ch| ch != b'}' && ch != b'^' && ch != b'"')
                        {
                            q += 1;
                        }
                        self.emit(T::ShapeData, p, q);
                        true
                    }
                }
                S::ShapeDataStr => {
                    if c == u32::from(b'"') {
                        self.pop();
                        self.emit(T::ShapeData, p, p + 1);
                    } else {
                        let e = self.find_byte(b'"', p);
                        let text = self.src.get(p..e).unwrap_or("");
                        self.tokens.push(Token {
                            kind: T::ShapeData,
                            start: p,
                            text: Cow::Owned(break_lines(text)),
                        });
                        self.p = e;
                    }
                    true
                }
            };

            if !ok {
                self.tokens.push(Token {
                    kind: T::Error,
                    start: self.p,
                    text: Cow::Borrowed(""),
                });
                return;
            }
        }

        self.tokens.push(Token {
            kind: T::End,
            start: n,
            text: Cow::Borrowed(""),
        });
    }
}

/// `text.replace(/\n\s*/g, '<br/>')`.
fn break_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\n' {
            out.push(c);
            continue;
        }
        out.push_str("<br/>");
        while chars.next_if(|&next| is_space(u32::from(next))).is_some() {}
    }
    out
}
