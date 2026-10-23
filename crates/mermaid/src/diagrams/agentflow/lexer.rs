//! The agentflow lexer: a state machine equivalent to Mermaid's Jison lexer, which also records
//! each token's span the way Jison counts it. Port of Pele's `src/diagrams/agentflow/lexer.ts`.
//!
//! Pele reads UTF-16 code units. This port reads UTF-8 bytes: every delimiter is ASCII, and
//! non-ASCII characters are decoded whole wherever a test (`\s`, `\p{L}`) looks at them, so token
//! boundaries match. Columns are counted in UTF-16 units. Each regular expression in Pele is a
//! hand-written matcher below.

use std::borrow::Cow;

use super::tokens::T;
use super::types::Loc;
use crate::util::chars::{is_space, is_word};
use crate::util::unicode::is_unicode_letter;

/// A token: its kind, its start as a UTF-8 offset, its text, and its span.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Token<'a> {
    /// The token kind.
    pub(crate) kind: T,
    /// Where the token starts and ends, as Jison counts lines and columns.
    pub(crate) loc: Loc,
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
    EdgeText,
    EllipseText,
    Initial,
    MdString,
    ShapeData,
    ShapeDataStr,
    String,
    Text,
    TrapText,
}

/// The code past the end of the source or inside a multi-byte character, which no test matches.
/// Pele's `charCodeAt` returns `NaN` there.
const NONE: u32 = u32::MAX;

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

/// Lexer state. Pele keeps these as closure variables of `tokenize`.
struct Lexer<'a> {
    at_next: Option<usize>,
    blank: Option<(usize, usize)>,
    bytes: &'a [u8],
    col: u32,
    dir_failed: usize,
    dir_next: Option<usize>,
    first_graph: bool,
    line: u32,
    line_end: Option<usize>,
    no_arrow: Option<usize>,
    no_link: Option<usize>,
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
        blank: None,
        bytes: src.as_bytes(),
        col: 0,
        dir_failed: 0,
        dir_next: src.find("direction"),
        first_graph: true,
        line: 1,
        line_end: None,
        no_arrow: None,
        no_link: None,
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

/// The UTF-16 length of `text`.
fn utf16_len(text: &str) -> u32 {
    u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX)
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

    fn slice(&self, from: usize, to: usize) -> &str {
        self.src.get(from..to).unwrap_or("")
    }

    /// Moves over one match. Jison counts `\r\n`, `\r`, and `\n` as one line break each, and
    /// after a break its column stops at the first U+2028 or U+2029.
    fn advance(&mut self, e: usize) {
        let mut brk = None;
        let mut k = self.p;
        while k < e {
            match self.bytes.get(k) {
                Some(b'\n') => {
                    self.line += 1;
                    brk = Some(k);
                }
                Some(b'\r') => {
                    if k + 1 < e && self.bytes.get(k + 1) == Some(&b'\n') {
                        k += 1;
                    }
                    self.line += 1;
                    brk = Some(k);
                }
                _ => {}
            }
            k += 1;
        }

        match brk {
            None => self.col += utf16_len(self.slice(self.p, e)),
            Some(brk) => {
                let rest = self.slice(brk + 1, e);
                let stop = rest.find(['\u{2028}', '\u{2029}']).unwrap_or(rest.len());
                self.col = utf16_len(rest.get(..stop).unwrap_or(""));
            }
        }
        self.p = e;
    }

    fn start_loc(&self) -> Loc {
        Loc {
            first_column: self.col,
            first_line: self.line,
            last_column: self.col,
            last_line: self.line,
        }
    }

    /// Emits the text up to `e`, which may hold line breaks.
    fn emit(&mut self, kind: T, e: usize) {
        let start = self.p;
        let loc = self.start_loc();
        self.advance(e);
        self.finish(kind, start, e, loc);
    }

    /// Emits a token that cannot hold a line break.
    fn emit_flat(&mut self, kind: T, e: usize) {
        let start = self.p;
        let loc = self.start_loc();
        self.col += utf16_len(self.slice(start, e));
        self.p = e;
        self.finish(kind, start, e, loc);
    }

    /// Emits a run that Jison matches one character at a time, where `\r\n` counts as two line
    /// breaks.
    fn emit_chars(&mut self, kind: T, e: usize) {
        let start = self.p;
        let loc = self.start_loc();
        let src = self.src;
        for c in src.get(start..e).unwrap_or("").chars() {
            if c == '\n' || c == '\r' {
                self.line += 1;
                self.col = 0;
            } else {
                self.col += u32::try_from(c.len_utf16()).unwrap_or(1);
            }
        }
        self.p = e;
        self.finish(kind, start, e, loc);
    }

    /// Emits a token with rewritten `text`, spanning up to `e`.
    fn emit_text(&mut self, kind: T, text: String, e: usize) {
        let start = self.p;
        let mut loc = self.start_loc();
        self.advance(e);
        loc.last_line = self.line;
        loc.last_column = self.col;
        self.tokens.push(Token {
            kind,
            loc,
            start,
            text: Cow::Owned(text),
        });
    }

    fn finish(&mut self, kind: T, start: usize, end: usize, mut loc: Loc) {
        loc.last_line = self.line;
        loc.last_column = self.col;
        let src = self.src;
        self.tokens.push(Token {
            kind,
            loc,
            start,
            text: Cow::Borrowed(src.get(start..end).unwrap_or("")),
        });
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

    /// The end of the run of whitespace starting at `q`. The last run is kept, so asking again
    /// from inside it costs nothing.
    fn ws_end(&mut self, q: usize) -> usize {
        if let Some((from, to)) = self.ws
            && q >= from
            && q <= to
        {
            return to;
        }
        let end = self.skip_space(q);
        self.ws = Some((q, end));
        end
    }

    /// The end of the run of spaces and tabs starting at `q`, kept like [`Self::ws_end`].
    fn blank_end(&mut self, q: usize) -> usize {
        if let Some((from, to)) = self.blank
            && q >= from
            && q <= to
        {
            return to;
        }
        let end = self.blanks(q);
        self.blank = Some((q, end));
        end
    }

    /// The end of `\s*--+[x>]\s*` starting at `q`.
    fn arrow_end(&mut self, q: usize) -> Option<usize> {
        let j = self.ws_end(q);
        if Some(j) == self.no_arrow
            || self.bytes.get(j) != Some(&b'-')
            || self.bytes.get(j + 1) != Some(&b'-')
        {
            return None;
        }
        let mut k = j + 2;
        while self.bytes.get(k) == Some(&b'-') {
            k += 1;
        }
        if matches!(self.bytes.get(k), Some(b'x' | b'>')) {
            return Some(self.ws_end(k + 1));
        }
        self.no_arrow = Some(j);
        None
    }

    fn link(&mut self) -> bool {
        let j = self.ws_end(self.p);
        if Some(j) == self.no_link || self.bytes.get(j) != Some(&b'-') {
            return false;
        }
        let d = self.bytes.get(j + 1).copied();
        let mut k = j + 2;
        if d == Some(b'-') {
            while self.bytes.get(k) == Some(&b'-') {
                k += 1;
            }
            if matches!(self.bytes.get(k), Some(b'x' | b'>')) {
                let e = self.ws_end(k + 1);
                self.emit(T::Link, e);
            } else {
                self.push(S::EdgeText);
                let e = self.ws_end(j + 2);
                self.emit(T::StartLink, e);
            }
            return true;
        }
        if d == Some(b'.') {
            while self.bytes.get(k) == Some(&b'.') {
                k += 1;
            }
            if self.bytes.get(k) == Some(&b'-') {
                let e = self.ws_end(k + 1);
                self.emit(T::Link, e);
                return true;
            }
        }
        self.no_link = Some(j);
        false
    }

    /// The end of a `%%` comment not followed by `{`, through the end of its line.
    fn comment_end(&self, q: usize) -> Option<usize> {
        if self.bytes.get(q) != Some(&b'%')
            || self.bytes.get(q + 1) != Some(&b'%')
            || self.bytes.get(q + 2) == Some(&b'{')
        {
            return None;
        }
        Some(self.find_byte(b'\n', q + 2))
    }

    fn quote(&mut self) {
        if self.byte(self.p + 1) == u32::from(b'`') {
            self.push(S::MdString);
            self.advance(self.p + 2);
        } else {
            self.push(S::String);
            self.advance(self.p + 1);
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

    /// `/\(\s*\)/y` (`RE_EMPTY_CALL`).
    fn re_empty_call(&self, q: usize) -> Option<usize> {
        if self.bytes.get(q) != Some(&b'(') {
            return None;
        }
        let i = self.skip_space(q + 1);
        (self.bytes.get(i) == Some(&b')')).then_some(i + 1)
    }

    fn opener(&mut self, c: u32) -> bool {
        let p = self.p;
        let d = self.byte(p + 1);
        match u8::try_from(c) {
            Ok(b'(') => {
                if d == u32::from(b'-') {
                    self.push(S::EllipseText);
                    self.emit_flat(T::EllipseStart, p + 2);
                } else if d == u32::from(b'[') {
                    self.push(S::Text);
                    self.emit_flat(T::StadiumStart, p + 2);
                } else if d == u32::from(b'(') && self.byte(p + 2) == u32::from(b'(') {
                    self.push(S::Text);
                    self.emit_flat(T::DoubleCircleStart, p + 3);
                } else {
                    self.push(S::Text);
                    self.emit_flat(T::Ps, p + 1);
                }
                true
            }
            Ok(b'[') => {
                if d == u32::from(b'[') {
                    self.push(S::Text);
                    self.emit_flat(T::SubroutineStart, p + 2);
                } else if d == u32::from(b'(') {
                    self.push(S::Text);
                    self.emit_flat(T::CylinderStart, p + 2);
                } else if d == u32::from(b'/') {
                    self.push(S::TrapText);
                    self.emit_flat(T::TrapStart, p + 2);
                } else if d == u32::from(b'\\') {
                    self.push(S::TrapText);
                    self.emit_flat(T::InvTrapStart, p + 2);
                } else {
                    self.push(S::Text);
                    self.emit_flat(T::Sqs, p + 1);
                }
                true
            }
            Ok(b'|') => {
                self.push(S::Text);
                self.emit_flat(T::Pipe, p + 1);
                true
            }
            Ok(b'{') => {
                self.push(S::Text);
                self.emit_flat(T::DiamondStart, p + 1);
                true
            }
            _ => false,
        }
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
        self.emit_flat(T::NodeString, q);
        true
    }

    /// The end of the run of spaces and tabs starting at `from`.
    fn blanks(&self, from: usize) -> usize {
        let mut e = from;
        while matches!(self.bytes.get(e), Some(b' ' | b'\t')) {
            e += 1;
        }
        e
    }

    fn keyword(&mut self, kind: T, len: usize) -> bool {
        self.emit_flat(kind, self.p + len);
        true
    }

    #[expect(
        clippy::too_many_lines,
        reason = "mirrors Pele's initial, one branch per rule"
    )]
    fn initial(&mut self, c: u32) -> bool {
        let p = self.p;
        match u8::try_from(c) {
            Ok(b'a') => {
                if self.starts_with("acc", p) {
                    if let Some(e) = self.re_acc("accTitle", b':') {
                        self.push(S::AccTitle);
                        self.emit(T::AccTitle, e);
                        return true;
                    }
                    if let Some(e) = self.re_acc("accDescr", b':') {
                        self.push(S::AccDescr);
                        self.emit(T::AccDescr, e);
                        return true;
                    }
                    if let Some(e) = self.re_acc("accDescr", b'{') {
                        self.push(S::AccDescrMultiline);
                        self.advance(e);
                        return true;
                    }
                }
                if self.kw("agentflow-beta") {
                    if self.first_graph {
                        self.first_graph = false;
                        self.push(S::Dir);
                    }
                    return self.keyword(T::Graph, 14);
                }
            }
            Ok(b'@') => {
                if self.byte(p + 1) == u32::from(b'{') {
                    self.push(S::ShapeData);
                    self.emit_text(T::ShapeData, String::new(), p + 2);
                    return true;
                }
            }
            Ok(b'c') => {
                if let Some(e) = self.re_word_space("call") {
                    self.push(S::CallbackName);
                    self.advance(e);
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
                    self.advance(e);
                    return true;
                }
                if self.kw("connector") {
                    return self.keyword(T::Connector, 9);
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
                    self.emit(T::Href, end);
                    return true;
                }
            }
            Ok(b'f') => {
                if self.kw("flow") {
                    return self.keyword(T::Flow, 4);
                }
            }
            Ok(b'g') => {
                if self.kw("global") {
                    let e = self.blanks(p + 6);
                    self.emit_flat(T::Global, e);
                    return true;
                }
            }
            Ok(b'e') => {
                if self.kw("end") {
                    let e = self.blanks(p + 3);
                    self.emit_flat(T::BlockEnd, e);
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
            self.emit_flat(T::Num, q);
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
            Ok(b'<') => return self.keyword(T::TagStart, 1),
            Ok(b'-') => {
                if self.link() || self.node_string() {
                    return true;
                }
                return self.keyword(T::Minus, 1);
            }
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
            Ok(b'%') => {
                if let Some(e) = self.comment_end(p) {
                    self.emit(T::Comment, e);
                    return true;
                }
                return self.node_string();
            }
            _ => {}
        }

        if is_space(c) {
            if self.link() {
                return true;
            }
            if c == 32 || c == 9 {
                let from = self.blank_end(p);
                if let Some(e) = self.comment_end(from) {
                    self.emit(T::Comment, e);
                    return true;
                }
            }
            let mut e = p;
            loop {
                match self.bytes.get(e) {
                    Some(b'\n') => e += 1,
                    Some(b'\r') if self.bytes.get(e + 1) == Some(&b'\n') => e += 2,
                    _ => break,
                }
            }
            if e > p {
                self.emit(T::Newline, e);
            } else {
                let end = self.next(p);
                self.emit(T::Space, end);
            }
            return true;
        }

        if c < 128 {
            return self.node_string();
        }
        if is_unicode_letter(c) {
            let end = self.next(p);
            self.emit_flat(T::UnicodeText, end);
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
                self.emit(kind, e);
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
            self.emit_flat(T::LinkId, at + 1);
            return true;
        }
        false
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
                    self.emit_flat(T::StadiumEnd, p + 2);
                } else if d == u32::from(b']') {
                    self.emit_flat(T::SubroutineEnd, p + 2);
                } else {
                    self.emit_flat(T::Sqe, p + 1);
                }
            }
            Ok(b')') => {
                self.pop();
                if d == u32::from(b']') {
                    self.emit_flat(T::CylinderEnd, p + 2);
                } else if d == u32::from(b')') && self.byte(p + 2) == u32::from(b')') {
                    self.emit_flat(T::DoubleCircleEnd, p + 3);
                } else {
                    self.emit_flat(T::Pe, p + 1);
                }
            }
            Ok(b'|') => {
                self.pop();
                self.emit_flat(T::Pipe, p + 1);
            }
            Ok(b'}') => {
                self.pop();
                self.emit_flat(T::DiamondStop, p + 1);
            }
            Ok(b'%') => {
                // Jison drops the lone `%` from Mermaid's text pattern, so only a comment may
                // start with one.
                let Some(e) = self.comment_end(p) else {
                    return false;
                };
                self.advance(e);
            }
            _ => {
                let mut q = p + 1;
                while let Some(&ch) = self.bytes.get(q) {
                    if matches!(
                        ch,
                        b'[' | b']' | b'(' | b')' | b'{' | b'}' | b'|' | b'"' | b'%'
                    ) {
                        break;
                    }
                    q += 1;
                }
                self.emit(T::Text, q);
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
        } else if let Some(e) = (c == u32::from(b'%'))
            .then(|| self.comment_end(p))
            .flatten()
        {
            self.advance(e);
        } else if matches!(u8::try_from(c), Ok(b'-' | b'/' | b')')) && d == u32::from(b')') {
            self.pop();
            self.emit_flat(T::EllipseEnd, p + 2);
        } else if matches!(u8::try_from(c), Ok(b'(' | b'[' | b'{')) {
            self.opener(c);
        } else if matches!(u8::try_from(c), Ok(b')' | b']' | b'}')) {
            return false;
        } else {
            let n = self.bytes.len();
            let mut q = p + 1;
            while q < n {
                let ch = self.bytes.get(q).copied().unwrap_or(0);
                if matches!(ch, b'"' | b'(' | b')' | b'[' | b']' | b'{' | b'}') {
                    break;
                }
                if matches!(ch, b'-' | b'/') && self.bytes.get(q + 1) == Some(&b')') {
                    break;
                }
                if ch == b'%'
                    && self.bytes.get(q + 1) == Some(&b'%')
                    && self.bytes.get(q + 2) != Some(&b'{')
                {
                    break;
                }
                q += 1;
            }
            self.emit_chars(T::Text, q);
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
                    self.emit_flat(T::TrapEnd, p + 2);
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
                    self.emit_flat(T::TrapEnd, p + 2);
                } else {
                    self.emit_flat(T::Text, p + 1);
                }
            }
            Ok(b'/') => {
                if d == close {
                    self.pop();
                    self.emit_flat(T::InvTrapEnd, p + 2);
                } else {
                    self.emit_flat(T::Text, p + 1);
                }
            }
            Ok(b']' | b')') => {
                if d != close {
                    return false;
                }
                self.pop();
                self.emit_flat(T::TrapEnd, p + 2);
            }
            Ok(b'}') => return false,
            Ok(b'%') => {
                let Some(e) = self.comment_end(p) else {
                    return false;
                };
                self.advance(e);
            }
            _ => {
                if (c == u32::from(b'?') || c == u32::from(b'=')) && d == close {
                    self.pop();
                    self.emit_flat(T::TrapEnd, p + 2);
                    return true;
                }
                let mut q = p + 1;
                while let Some(&ch) = self.bytes.get(q) {
                    if matches!(
                        ch,
                        b'\\' | b'[' | b']' | b'(' | b')' | b'{' | b'}' | b'/' | b'%'
                    ) {
                        break;
                    }
                    q += 1;
                }
                self.emit(T::Text, q);
            }
        }
        true
    }

    fn edge_text(&mut self, c: u32) -> bool {
        if c == u32::from(b'"') {
            self.quote();
            return true;
        }

        let p = self.p;
        if let Some(e) = self.arrow_end(p) {
            self.pop();
            self.emit(T::Link, e);
            return true;
        }
        if c == u32::from(b'-') && self.byte(p + 1) == u32::from(b'-') {
            return false;
        }

        let n = self.bytes.len();
        let mut q = self.next(p);
        while q < n {
            let ch = self.code(q);
            if ch == u32::from(b'"') {
                break;
            }
            if ch == u32::from(b'-') {
                if self.byte(q + 1) == u32::from(b'-') {
                    break;
                }
            } else if is_space(ch) && self.arrow_end(q).is_some() {
                break;
            }
            q = self.next(q);
        }
        self.emit_chars(T::EdgeText, q);
        true
    }

    #[expect(
        clippy::too_many_lines,
        reason = "mirrors Pele's lexing loop, one branch per start condition"
    )]
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
                    self.emit_flat(kind, n);
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
                            self.advance(p + 2);
                        } else {
                            self.pop();
                            self.advance(p + 1);
                        }
                    } else {
                        let e = self.find_byte(b'"', p);
                        self.emit(T::Str, e);
                    }
                    true
                }
                S::MdString => {
                    if c == u32::from(b'`') {
                        if self.byte(p + 1) == u32::from(b'"') {
                            self.pop();
                            self.advance(p + 2);
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
                        self.emit(T::MdStr, q);
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
                    self.emit(kind, e);
                    true
                }
                S::AccDescrMultiline => {
                    if c == u32::from(b'}') {
                        self.pop();
                        self.advance(p + 1);
                    } else {
                        let e = self.find_byte(b'}', p);
                        self.emit(T::AccDescrMultilineValue, e);
                    }
                    true
                }
                S::Dir => {
                    if c == u32::from(b'"') {
                        self.quote();
                        true
                    } else if let Some(e) = self.re_nodir(p) {
                        self.pop();
                        self.emit(T::NoDir, e);
                        true
                    } else if let Some(e) = self
                        .re_dir(p)
                        .or_else(|| self.re_dir_sym(p))
                        .or_else(|| self.re_dir_v(p))
                    {
                        self.pop();
                        self.emit(T::Dir, e);
                        true
                    } else {
                        self.opener(c)
                    }
                }
                S::Text => self.text_state(c),
                S::EllipseText => self.ellipse_text(c),
                S::TrapText => self.trap_text(c),
                S::EdgeText => self.edge_text(c),
                S::Click => {
                    if c == u32::from(b'"') {
                        self.quote();
                    } else if is_space(c) {
                        self.pop();
                        let end = self.next(p);
                        self.advance(end);
                    } else {
                        let mut q = p + 1;
                        while q < n && !is_space(self.code(q)) {
                            q += 1;
                        }
                        self.emit_flat(T::Click, q);
                    }
                    true
                }
                S::CallbackName => {
                    if c == u32::from(b'(') {
                        let e = self.re_empty_call(p);
                        self.pop();
                        if let Some(e) = e {
                            self.advance(e);
                        } else {
                            self.push(S::CallbackArgs);
                            self.advance(p + 1);
                        }
                    } else {
                        let e = self.find_byte(b'(', p);
                        self.emit(T::CallbackName, e);
                    }
                    true
                }
                S::CallbackArgs => {
                    if c == u32::from(b')') {
                        self.pop();
                        self.advance(p + 1);
                    } else {
                        let e = self.find_byte(b')', p);
                        self.emit(T::CallbackArgs, e);
                    }
                    true
                }
                S::ShapeData => {
                    if c == u32::from(b'"') {
                        self.push(S::ShapeDataStr);
                        self.emit_flat(T::ShapeData, p + 1);
                        true
                    } else if c == u32::from(b'}') {
                        self.pop();
                        self.advance(p + 1);
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
                        self.emit(T::ShapeData, q);
                        true
                    }
                }
                S::ShapeDataStr => {
                    if c == u32::from(b'"') {
                        self.pop();
                        self.emit_flat(T::ShapeData, p + 1);
                    } else {
                        let e = self.find_byte(b'"', p);
                        let text = break_lines(self.slice(p, e));
                        self.emit_text(T::ShapeData, text, e);
                    }
                    true
                }
            };

            if !ok {
                let loc = self.start_loc();
                self.tokens.push(Token {
                    kind: T::Error,
                    loc,
                    start: self.p,
                    text: Cow::Borrowed(""),
                });
                return;
            }
        }

        let loc = self.start_loc();
        self.tokens.push(Token {
            kind: T::End,
            loc,
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
