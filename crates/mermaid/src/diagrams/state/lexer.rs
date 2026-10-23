//! The state lexer: a state machine equivalent to Mermaid's Jison lexer. Port of Pele's
//! `src/diagrams/state/lexer.ts`.
//!
//! Pele reads UTF-16 code units. This port reads UTF-8 bytes: every delimiter is ASCII, and
//! non-ASCII characters are decoded whole wherever `\s` looks at them, so token boundaries match.

use super::tokens::T;
use crate::text::label::trim;
use crate::util::chars::{is_space, is_word};

/// A token: its kind, its start as a UTF-8 offset, and its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token<'a> {
    /// The token kind.
    pub(crate) kind: T,
    /// The UTF-8 offset of the token in the source.
    pub(crate) start: usize,
    /// The token text.
    pub(crate) text: &'a str,
}

/// The tokens of a source.
pub(crate) struct Tokens<'a> {
    /// Set when lexing stopped on the grammar's own error rather than on unrecognized text.
    pub(crate) error: Option<String>,
    /// The tokens, ending with `End` or `Error`.
    pub(crate) tokens: Vec<Token<'a>>,
}

/// Lexer start conditions, as in Mermaid's grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum S {
    AccDescr,
    AccDescrMultiline,
    AccTitle,
    Class,
    ClassDef,
    ClassDefId,
    ClassStyle,
    FloatingNote,
    FloatingNoteId,
    Initial,
    Note,
    NoteId,
    NoteText,
    Scale,
    State,
    StateId,
    StateString,
    Struct,
    Style,
    StyleDefStyles,
}

/// The code past the end of the source or inside a multi-byte character, which no test matches.
/// Pele's `charCodeAt` returns `NaN` there.
const NONE: u32 = u32::MAX;

/// The direction tokens, in the grammar's rule order.
const DIRECTIONS: [T; 4] = [
    T::DirectionTb,
    T::DirectionBt,
    T::DirectionRl,
    T::DirectionLr,
];

/// Pele's `finder`: the next match of a pattern at or after a position, remembered so that asking
/// at each token costs one scan of the text in total.
#[derive(Default)]
struct Finder {
    /// Where the remembered answer was searched from.
    from: usize,
    /// The remembered answer: the match, or `None` for no match.
    next: Option<usize>,
    /// Whether there is a remembered answer.
    searched: bool,
}

impl Finder {
    fn get(&mut self, q: usize, find: impl Fn(usize) -> Option<usize>) -> Option<usize> {
        if !self.searched || q < self.from || self.next.is_some_and(|next| q > next) {
            self.next = find(q);
            self.from = q;
            self.searched = true;
        }
        self.next
    }
}

/// Lexer state. Pele keeps these as closure variables of `tokenize`.
struct Lexer<'a> {
    bytes: &'a [u8],
    brace: Finder,
    dir_at: [Option<usize>; 4],
    dir_end: [usize; 4],
    dir_from: Option<usize>,
    dir_to: usize,
    direction: Finder,
    error: Option<String>,
    line_end: Finder,
    p: usize,
    percent: Finder,
    src: &'a str,
    stack: Vec<S>,
    state: S,
    stereo_at: [Option<usize>; 6],
    stereo_from: Option<usize>,
    stereo_to: usize,
    stereotype: Finder,
    tokens: Vec<Token<'a>>,
}

/// Splits `src` into tokens. The list ends with an `End` token, or with an `Error` token where
/// no rule matches.
pub(crate) fn tokenize(src: &str) -> Tokens<'_> {
    let mut lexer = Lexer {
        bytes: src.as_bytes(),
        brace: Finder::default(),
        dir_at: [None; 4],
        dir_end: [0; 4],
        dir_from: None,
        dir_to: 0,
        direction: Finder::default(),
        error: None,
        line_end: Finder::default(),
        p: 0,
        percent: Finder::default(),
        src,
        stack: Vec::new(),
        state: S::Initial,
        stereo_at: [None; 6],
        stereo_from: None,
        stereo_to: 0,
        stereotype: Finder::default(),
        tokens: Vec::new(),
    };
    lexer.run();
    Tokens {
        error: lexer.error,
        tokens: lexer.tokens,
    }
}

/// The first `direction` at or after `q`, ignoring case.
fn find_direction(bytes: &[u8], q: usize) -> Option<usize> {
    (q..bytes.len()).find(|&i| {
        bytes
            .get(i..i + 9)
            .is_some_and(|w| w.eq_ignore_ascii_case(b"direction"))
    })
}

/// The kind of stereotype at `i`, as an index into the lexer's `stereo_at`: `<<fork>>`,
/// `<<join>>`, `<<choice>>`, then the same in `[[ ]]`, ignoring case.
fn stereotype_at(bytes: &[u8], i: usize) -> Option<usize> {
    let (open, close, base) = match bytes.get(i) {
        Some(b'<') => (b'<', b">>", 0),
        Some(b'[') => (b'[', b"]]", 3),
        _ => return None,
    };
    if bytes.get(i + 1) != Some(&open) {
        return None;
    }
    for (kind, word) in [&b"fork"[..], b"join", b"choice"].into_iter().enumerate() {
        let from = i + 2;
        let to = from + word.len();
        if bytes
            .get(from..to)
            .is_some_and(|w| w.eq_ignore_ascii_case(word))
            && bytes.get(to..to + 2) == Some(close)
        {
            return Some(base + kind);
        }
    }
    None
}

/// The first line terminator (`\n`, `\r`, U+2028, U+2029) at or after `q`.
fn find_line_end(bytes: &[u8], q: usize) -> Option<usize> {
    (q..bytes.len()).find(|&i| match bytes.get(i) {
        Some(b'\n' | b'\r') => true,
        Some(0xe2) => {
            bytes.get(i + 1) == Some(&0x80) && matches!(bytes.get(i + 2), Some(0xa8 | 0xa9))
        }
        _ => false,
    })
}

impl<'a> Lexer<'a> {
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

    fn emit(&mut self, kind: T, s: usize, e: usize) {
        let text = self.src.get(s..e).unwrap_or("");
        self.emit_text(kind, text, s, e);
    }

    fn emit_text(&mut self, kind: T, text: &'a str, s: usize, e: usize) {
        self.tokens.push(Token {
            kind,
            start: s,
            text,
        });
        self.p = e;
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

    /// Case-insensitive match of a lowercase ASCII word, as the grammar's `/i` flag gives.
    fn word(&self, w: &str, q: usize) -> bool {
        w.bytes().enumerate().all(|(i, a)| {
            let c = self.byte(q + i);
            c == u32::from(a) || (a.is_ascii_lowercase() && c == u32::from(a) - 32)
        })
    }

    fn ws_end(&self, q: usize) -> usize {
        let mut j = q;
        while j < self.bytes.len() && is_space(self.code(j)) {
            j = self.next(j);
        }
        j
    }

    fn word_end(&self, q: usize) -> usize {
        let mut j = q;
        while is_word(self.byte(j)) {
            j += 1;
        }
        j
    }

    /// The next `\n` at or after `q`, or the end.
    fn eol(&self, q: usize) -> usize {
        self.src
            .get(q..)
            .and_then(|rest| rest.find('\n'))
            .map_or(self.bytes.len(), |k| q + k)
    }

    fn find_char(&self, q: usize, c: char) -> Option<usize> {
        self.src
            .get(q..)
            .and_then(|rest| rest.find(c))
            .map(|k| q + k)
    }

    fn line_end(&mut self, q: usize) -> usize {
        let bytes = self.bytes;
        self.line_end
            .get(q, |q| find_line_end(bytes, q))
            .unwrap_or(bytes.len())
    }

    /// Rules `.*direction\s+TB[^\n]*` and its three siblings: the first kind, in grammar order,
    /// found between `q` and the end of its line.
    fn direction(&mut self, q: usize) -> Option<usize> {
        if self.dir_from.is_none_or(|from| q < from) || q > self.dir_to {
            let bytes = self.bytes;
            let first = self.direction.get(q, |q| find_direction(bytes, q));
            let end = if first.is_none() {
                bytes.len()
            } else {
                self.line_end(q)
            };
            self.dir_from = Some(q);
            self.dir_to = end;
            self.dir_at = [None; 4];
            let mut d = first;
            while let Some(at) = d.filter(|&at| at < end) {
                let w = self.ws_end(at + 9);
                if w != at + 9 {
                    let kind = ["tb", "bt", "rl", "lr"]
                        .iter()
                        .position(|dir| self.word(dir, w));
                    if let Some(kind) = kind
                        && let (Some(slot), Some(slot_end)) =
                            (self.dir_at.get_mut(kind), self.dir_end.get_mut(kind))
                    {
                        *slot = Some(at);
                        *slot_end = w + 2;
                    }
                }
                d = self.direction.get(at + 1, |q| find_direction(bytes, q));
            }
        }
        self.dir_at
            .iter()
            .position(|at| at.is_some_and(|at| at >= q))
    }

    fn stereotype(&mut self, q: usize) -> Option<usize> {
        if self.stereo_from.is_none_or(|from| q < from) || q > self.stereo_to {
            let bytes = self.bytes;
            let find = |q: usize| (q..bytes.len()).find(|&i| stereotype_at(bytes, i).is_some());
            let first = self.stereotype.get(q, find);
            let end = if first.is_none() {
                bytes.len()
            } else {
                self.line_end(q)
            };
            self.stereo_from = Some(q);
            self.stereo_to = end;
            self.stereo_at = [None; 6];
            let mut d = first;
            while let Some(at) = d.filter(|&at| at < end) {
                if let Some(slot) =
                    stereotype_at(bytes, at).and_then(|kind| self.stereo_at.get_mut(kind))
                {
                    *slot = Some(at);
                }
                d = self.stereotype.get(at + 1, find);
            }
        }
        self.stereo_at
            .iter()
            .position(|at| at.is_some_and(|at| at >= q))
    }

    /// The grammar's `processId()`: an id ends at an inline `%%`, and a text that starts with one
    /// is dropped.
    fn emit_id(&mut self, s: usize, e: usize) -> bool {
        let src = self.src;
        let at = self.percent.get(s, |q| {
            src.get(q..).and_then(|rest| rest.find("%%")).map(|k| q + k)
        });
        match at {
            Some(at) if at + 2 <= e => {
                if at == s {
                    self.p = e;
                    return false;
                }
                self.emit(T::Id, s, at);
                true
            }
            _ => {
                self.emit(T::Id, s, e);
                true
            }
        }
    }

    fn finish(&mut self, kind: T) {
        let start = self.p;
        self.tokens.push(Token {
            kind,
            start,
            text: "",
        });
    }

    fn run(&mut self) {
        loop {
            if !self.step() {
                return;
            }
        }
    }

    /// Lexes one token or skips one stretch. Returns false once the token list is finished.
    #[expect(
        clippy::too_many_lines,
        clippy::many_single_char_names,
        reason = "mirrors Pele's lexer switch and names, one arm per start condition"
    )]
    fn step(&mut self) -> bool {
        let n = self.bytes.len();
        let p = self.p;
        if p >= n {
            // Jison tries the current state's rules once more on the empty remainder.
            let kind = match self.state {
                S::Initial => Some(T::Nl),
                S::AccTitle => Some(T::AccTitleValue),
                S::AccDescr => Some(T::AccDescrValue),
                S::AccDescrMultiline => Some(T::AccDescrMultilineValue),
                S::ClassDefId => Some(T::ClassDefStyleOpts),
                S::ClassStyle => Some(T::StyleClass),
                S::StyleDefStyles => Some(T::StyleDefStyleOpts),
                S::StateString => Some(T::StateDescr),
                S::FloatingNote => Some(T::NoteText),
                S::StateId | S::FloatingNoteId => Some(T::Id),
                _ => None,
            };
            if let Some(kind) = kind {
                self.emit(kind, n, n);
            }
            self.finish(T::End);
            return false;
        }

        let c = self.code(p);
        match self.state {
            S::Initial | S::Struct => self.step_document(c),
            S::State => self.step_state(c),
            S::StateString => {
                if c == 34 {
                    self.p += 1;
                    self.pop();
                    return true;
                }
                let close = self.find_char(p, '"').unwrap_or(n);
                self.emit(T::StateDescr, p, close);
                true
            }
            S::StateId => {
                let mut j = p;
                while j < n && !matches!(self.byte(j), 10 | 123) {
                    j += 1;
                }
                if self.emit_id(p, j) {
                    self.pop();
                }
                true
            }
            S::Scale => {
                if (48..=57).contains(&c) {
                    let mut j = p + 1;
                    while (48..=57).contains(&self.byte(j)) {
                        j += 1;
                    }
                    self.emit(T::Width, p, j);
                    return true;
                }
                let q = self.ws_end(p);
                if q > p && self.word("width", q) && !is_word(self.byte(q + 5)) {
                    self.p = q + 5;
                    self.pop();
                    return true;
                }
                self.finish(T::Error);
                false
            }
            S::AccTitle => {
                let e = self.eol(p);
                self.emit(T::AccTitleValue, p, e);
                self.pop();
                true
            }
            S::AccDescr => {
                let e = self.eol(p);
                self.emit(T::AccDescrValue, p, e);
                self.pop();
                true
            }
            S::AccDescrMultiline => {
                if c == 125 {
                    self.p += 1;
                    self.pop();
                    return true;
                }
                let close = self.find_char(p, '}').unwrap_or(n);
                self.emit(T::AccDescrMultilineValue, p, close);
                true
            }
            S::ClassDef => {
                let is_default = self.word("default", p) && self.ws_end(p + 7) > p + 7;
                let j = if is_default { p + 7 } else { self.word_end(p) };
                let e = self.ws_end(j);
                if j == p || e == j {
                    self.finish(T::Error);
                    return false;
                }
                self.emit(
                    if is_default {
                        T::DefaultClassDefId
                    } else {
                        T::ClassDefId
                    },
                    p,
                    e,
                );
                self.pop();
                self.push(S::ClassDefId);
                true
            }
            S::ClassDefId => {
                let e = self.eol(p);
                self.emit(T::ClassDefStyleOpts, p, e);
                self.pop();
                true
            }
            S::Class => {
                let mut j = self.word_end(p);
                if j == p {
                    self.finish(T::Error);
                    return false;
                }
                while self.byte(j) == 44 {
                    let k = self.ws_end(j + 1);
                    let e = self.word_end(k);
                    if e == k {
                        break;
                    }
                    j = e;
                }
                self.emit(T::ClassEntityIds, p, j);
                self.pop();
                self.push(S::ClassStyle);
                true
            }
            S::ClassStyle => {
                let e = self.eol(p);
                self.emit(T::StyleClass, p, e);
                self.pop();
                true
            }
            S::Style => {
                let mut j = p;
                while j < n && (is_word(self.byte(j)) || self.byte(j) == 44) {
                    j += 1;
                }
                let e = self.ws_end(j);
                if j == p || e == j {
                    self.finish(T::Error);
                    return false;
                }
                self.emit(T::StyleIds, p, e);
                self.pop();
                self.push(S::StyleDefStyles);
                true
            }
            S::StyleDefStyles => {
                let e = self.eol(p);
                self.emit(T::StyleDefStyleOpts, p, e);
                self.pop();
                true
            }
            S::Note => {
                if self.word("left of", p) && !is_word(self.byte(p + 7)) {
                    self.emit(T::LeftOf, p, p + 7);
                    self.pop();
                    self.push(S::NoteId);
                    return true;
                }
                if self.word("right of", p) && !is_word(self.byte(p + 8)) {
                    self.emit(T::RightOf, p, p + 8);
                    self.pop();
                    self.push(S::NoteId);
                    return true;
                }
                if c == 34 {
                    self.p += 1;
                    self.pop();
                    self.push(S::FloatingNote);
                    return true;
                }
                self.finish(T::Error);
                false
            }
            S::NoteId => {
                let q = self.ws_end(p);
                let mut j = q;
                while j < n {
                    let d = self.code(j);
                    if d == 58 || d == 45 || is_space(d) {
                        break;
                    }
                    j = self.next(j);
                }
                if j == q {
                    self.finish(T::Error);
                    return false;
                }
                if self.emit_id(p, j) {
                    self.pop();
                    self.push(S::NoteText);
                }
                true
            }
            S::NoteText => self.step_note_text(),
            S::FloatingNote => {
                let q = self.ws_end(p);
                if self.word("as", q) {
                    let e = self.ws_end(q + 2);
                    self.emit(T::As, p, e);
                    self.pop();
                    self.push(S::FloatingNoteId);
                    return true;
                }
                if c == 34 {
                    self.p += 1;
                    return true;
                }
                let close = self.find_char(p, '"').unwrap_or(n);
                self.emit(T::NoteText, p, close);
                true
            }
            S::FloatingNoteId => {
                let e = self.eol(p);
                if self.emit_id(p, e) {
                    self.pop();
                }
                true
            }
        }
    }

    /// The rules of the root document (`INITIAL`) and of a composite's body (`struct`).
    #[expect(
        clippy::too_many_lines,
        clippy::many_single_char_names,
        reason = "mirrors Pele's rule order and names for the two document states"
    )]
    fn step_document(&mut self, c: u32) -> bool {
        let n = self.bytes.len();
        let p = self.p;
        let root = self.state == S::Initial;
        let lc = c | 32;

        if root {
            if lc == 99 && self.word("click", p) && !is_word(self.byte(p + 5)) {
                self.emit(T::Click, p, p + 5);
                return true;
            }
            if lc == 104 && self.word("href", p) && !is_word(self.byte(p + 4)) {
                self.emit(T::Href, p, p + 4);
                return true;
            }
            if c == 34
                && let Some(close) = self.find_char(p + 1, '"')
            {
                self.emit(T::String, p, close + 1);
                return true;
            }
            if lc == 100 && self.word("default", p) && !is_word(self.byte(p + 7)) {
                self.emit(T::Default, p, p + 7);
                return true;
            }
            if self.emit_direction(p) {
                return true;
            }
            if c == 10 {
                let mut j = p + 1;
                while self.byte(j) == 10 {
                    j += 1;
                }
                self.emit(T::Nl, p, j);
                return true;
            }
            if is_space(c) {
                self.p = self.ws_end(p);
                return true;
            }
        } else if c != 10 && is_space(c) {
            let mut j = self.next(p);
            while j < n && self.byte(j) != 10 && is_space(self.code(j)) {
                j = self.next(j);
            }
            self.p = j;
            return true;
        }

        if c == 35 || (c == 37 && self.byte(p + 1) == 37 && self.byte(p + 2) != 123) {
            self.p = self.eol(p);
            return true;
        }

        if lc == 115 {
            if root && self.word("scale", p) {
                let e = self.ws_end(p + 5);
                if e > p + 5 {
                    self.emit(T::Scale, p, e);
                    self.push(S::Scale);
                    return true;
                }
            }
            if self.word("style", p) {
                let e = self.ws_end(p + 5);
                if e > p + 5 {
                    self.emit(T::Style, p, e);
                    self.push(S::Style);
                    return true;
                }
            }
            if self.word("state", p) {
                let e = self.ws_end(p + 5);
                if e > p + 5 {
                    self.p = e;
                    self.push(S::State);
                    return true;
                }
                if root && self.word("diagram", p + 5) {
                    let from = if self.word("-v2", p + 12) {
                        p + 15
                    } else {
                        p + 12
                    };
                    let e = self.ws_end(from);
                    if e > from {
                        self.emit(T::Sd, p, e);
                        return true;
                    }
                }
            }
        } else if lc == 99 {
            if self.word("class", p) {
                let def = self.word("def", p + 5);
                let from = if def { p + 8 } else { p + 5 };
                let e = self.ws_end(from);
                if e > from {
                    self.emit(if def { T::ClassDef } else { T::Class }, p, e);
                    self.push(if def { S::ClassDef } else { S::Class });
                    return true;
                }
            }
        } else if lc == 97 && root && (self.word("acctitle", p) || self.word("accdescr", p)) {
            let title = self.byte(p + 3) | 32 == 116;
            let q = self.ws_end(p + 8);
            let d = self.byte(q);
            if d == 58 {
                let e = self.ws_end(q + 1);
                self.emit(if title { T::AccTitle } else { T::AccDescr }, p, e);
                self.push(if title { S::AccTitle } else { S::AccDescr });
                return true;
            }
            if d == 123 && !title {
                self.p = self.ws_end(q + 1);
                self.push(S::AccDescrMultiline);
                return true;
            }
        }

        if root {
            if c == 123 {
                self.emit(T::StructStart, p, p + 1);
                self.push(S::Struct);
                return true;
            }
        } else {
            if self.emit_direction(p) {
                return true;
            }
            if c == 125 {
                self.emit(T::StructStop, p, p + 1);
                self.pop();
                return true;
            }
            if c == 10 {
                self.p += 1;
                return true;
            }
        }

        if lc == 110 && self.word("note", p) {
            let e = self.ws_end(p + 4);
            if e > p + 4 {
                self.emit(T::Note, p, e);
                self.push(S::Note);
                return true;
            }
        }
        if root
            && lc == 104
            && self.word("hide empty description", p)
            && !is_word(self.byte(p + 22))
        {
            self.emit(T::HideEmpty, p, p + 22);
            return true;
        }
        if c == 91 && self.byte(p + 1) == 42 && self.byte(p + 2) == 93 {
            self.emit(T::EdgeState, p, p + 3);
            return true;
        }

        let mut j = p;
        while j < n {
            let d = self.code(j);
            if d == 58 || d == 45 || d == 123 || is_space(d) {
                break;
            }
            j = self.next(j);
        }
        if j > p {
            self.emit_id(p, j);
            return true;
        }

        if c == 58 {
            j = p + 1;
            while j < n {
                let d = self.byte(j);
                if d == 10 || d == 59 {
                    break;
                }
                if d == 58 {
                    let after = self.byte(j + 1);
                    if j + 1 >= n || after == 58 || after == 10 || after == 59 {
                        break;
                    }
                    j += 1;
                }
                j += 1;
            }
            if j > p + 1 {
                let text = trim(self.src.get(p..j).unwrap_or(""));
                self.emit_text(T::Descr, text, p, j);
                return true;
            }
            if self.byte(p + 1) == 58 && self.byte(p + 2) == 58 {
                self.emit(T::StyleSeparator, p, p + 3);
                return true;
            }
        } else if c == 45 && self.byte(p + 1) == 45 {
            if self.byte(p + 2) == 62 {
                self.emit(T::Arrow, p, p + 3);
                return true;
            }
            if !root {
                self.emit(T::Concurrent, p, p + 2);
                return true;
            }
        }

        if !root {
            self.finish(T::Error);
            return false;
        }
        self.emit(T::Invalid, p, p + 1);
        true
    }

    /// Emits a direction token at `p` when one of the direction rules matches.
    fn emit_direction(&mut self, p: usize) -> bool {
        let Some(kind) = self.direction(p) else {
            return false;
        };
        let (Some(&token), Some(&end)) = (DIRECTIONS.get(kind), self.dir_end.get(kind)) else {
            return false;
        };
        let e = self.eol(end);
        self.emit(token, p, e);
        true
    }

    /// The rules after `state `: a stereotype, a quoted description, `as`, a body, or a name.
    #[expect(clippy::many_single_char_names, reason = "keeps Pele's names")]
    fn step_state(&mut self, c: u32) -> bool {
        let n = self.bytes.len();
        let p = self.p;
        if c != 10 && is_space(c) {
            let mut j = self.next(p);
            while j < n && self.byte(j) != 10 && is_space(self.code(j)) {
                j = self.next(j);
            }
            self.p = j;
            return true;
        }
        if c == 35 || (c == 37 && self.byte(p + 1) == 37 && self.byte(p + 2) != 123) {
            self.p = self.eol(p);
            return true;
        }

        if let Some(kind) = self.stereotype(p)
            && let Some(&Some(at)) = self.stereo_at.get(kind)
        {
            let choice = kind == 2 || kind == 5;
            let token = if choice {
                T::Choice
            } else if kind % 3 == 0 {
                T::Fork
            } else {
                T::Join
            };
            let text = trim(self.src.get(p..at).unwrap_or(""));
            self.emit_text(token, text, p, at + if choice { 10 } else { 8 });
            self.pop();
            return true;
        }

        if c == 34 {
            self.p += 1;
            self.push(S::StateString);
            return true;
        }
        let q = if c == 10 { self.ws_end(p) } else { p };
        if self.word("as", q) {
            let e = self.ws_end(q + 2);
            if e > q + 2 {
                self.emit(T::As, p, e);
                self.push(S::StateId);
                return true;
            }
        }
        if c == 10 {
            self.p += 1;
            self.pop();
            return true;
        }
        if c == 123 {
            self.emit(T::StructStart, p, p + 1);
            self.pop();
            self.push(S::Struct);
            return true;
        }

        if is_word(c) {
            let first = self.word_end(p);
            let second = self.ws_end(first);
            if second > first && is_word(self.byte(second)) {
                let src = self.src;
                let brace = self.brace.get(second + 1, |q| {
                    src.get(q..).and_then(|rest| rest.find('{')).map(|k| q + k)
                });
                if let Some(brace) = brace
                    && brace < self.line_end(second)
                {
                    let mut error =
                        String::from("Error: State name must be a single word. Found: \"");
                    error.push_str(trim(self.src.get(p..=brace).unwrap_or("")));
                    error.push('"');
                    self.error = Some(error);
                    self.finish(T::Error);
                    return false;
                }
            }
        }

        let mut j = self.next(p);
        while j < n {
            let d = self.code(j);
            if d == 123 || is_space(d) {
                break;
            }
            j = self.next(j);
        }
        self.emit(T::CompositState, p, j);
        true
    }

    /// The text of a note beside a state: after a colon on one line, or up to `end note`.
    #[expect(clippy::many_single_char_names, reason = "keeps Pele's names")]
    fn step_note_text(&mut self) -> bool {
        let n = self.bytes.len();
        let p = self.p;
        let q = self.ws_end(p);
        if self.byte(q) == 58 {
            let mut j = q + 1;
            while j < n && !matches!(self.byte(j), 58 | 10 | 59) {
                j += 1;
            }
            if j > q + 1 {
                // Mermaid drops two characters here, expecting " :"; with no space before the
                // colon the first character of the note goes too.
                let from = self.next(self.next(p)).min(j);
                let text = trim(self.src.get(from..j).unwrap_or(""));
                self.emit_text(T::NoteText, text, p, j);
                self.pop();
                return true;
            }
        }

        let mut k = self.find_char(p, '\n');
        let mut e = 0;
        while let Some(at) = k {
            e = self.ws_end(at);
            if self.word("end note", e) && !is_word(self.byte(e + 8)) {
                break;
            }
            k = self.find_char(e, '\n');
        }
        if k.is_none() {
            self.finish(T::Error);
            return false;
        }
        let text = trim(self.src.get(p..e).unwrap_or(""));
        self.emit_text(T::NoteText, text, p, e + 8);
        self.pop();
        true
    }
}
