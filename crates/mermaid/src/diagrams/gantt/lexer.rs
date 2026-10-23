//! Splits gantt source into tokens with Mermaid's lexer rules. Port of Pele's
//! `src/diagrams/gantt/lexer.ts`.

use super::tokens::{T, WEEKDAYS};
use crate::util::chars::{is_digit, is_space, is_word};

/// A token: its kind, text, and UTF-8 offset in the source.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Token<'a> {
    /// Where the token starts.
    pub(crate) start: usize,
    /// The token's text.
    pub(crate) text: &'a str,
    /// The token's kind.
    pub(crate) r#type: T,
}

/// Lexer states, as the grammar names them.
#[derive(Clone, Copy, PartialEq)]
enum S {
    /// After `accDescr:`.
    AccDescr,
    /// Inside `accDescr { ... }`.
    AccDescrMultiline,
    /// After `accTitle:`.
    AccTitle,
    /// Inside the parentheses of a callback.
    Callbackargs,
    /// After `call`.
    Callbackname,
    /// After `click`.
    Click,
    /// Inside `href "..."`.
    Href,
    /// Between statements.
    Initial,
}

/// The weekday names, from Monday.
const DAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

/// What follows a keyword: `\b`, or one `\s` and then text up to one of the stop characters.
#[derive(Clone, Copy)]
enum Tail {
    /// A word boundary.
    Bound,
    /// Text up to one of the given bytes.
    Text(&'static [u8]),
}

/// Keyword statements by their first letter, in the grammar's rule order.
const KEYWORDS: [(u8, &str, Tail, T); 12] = [
    (b'a', "axisFormat", Tail::Text(b"#\n;"), T::AxisFormat),
    (
        b'a',
        "accDescription",
        Tail::Text(b"#\n;"),
        T::AccDescription,
    ),
    (b'g', "gantt", Tail::Bound, T::Gantt),
    (b'd', "dateFormat", Tail::Text(b"#\n;"), T::DateFormat),
    (b'i', "inclusiveEndDates", Tail::Bound, T::InclusiveEndDates),
    (b'i', "includes", Tail::Text(b"#\n;"), T::Includes),
    (b't', "topAxis", Tail::Bound, T::TopAxis),
    (b't', "tickInterval", Tail::Text(b"#\n;"), T::TickInterval),
    (b't', "todayMarker", Tail::Text(b"\n;"), T::TodayMarker),
    (b't', "title", Tail::Text(b"\n"), T::Title),
    (b'e', "excludes", Tail::Text(b"#\n;"), T::Excludes),
    (b's', "section", Tail::Text(b"\n"), T::Section),
];

/// The source being read.
struct Lexer<'a> {
    /// The current byte offset.
    p: usize,
    /// The source.
    src: &'a str,
    /// The lexer state.
    state: S,
    /// The tokens read so far.
    tokens: Vec<Token<'a>>,
}

impl Lexer<'_> {
    /// The character at byte `at`.
    fn char_at(&self, at: usize) -> Option<char> {
        self.src.get(at..).and_then(|rest| rest.chars().next())
    }

    /// The byte at `at`.
    fn byte(&self, at: usize) -> Option<u8> {
        self.src.as_bytes().get(at).copied()
    }

    /// Adds a token from `start` to `end` and moves past it.
    fn emit(&mut self, r#type: T, start: usize, end: usize) {
        self.tokens.push(Token {
            start,
            text: self.src.get(start..end).unwrap_or(""),
            r#type,
        });
        self.p = end;
    }

    /// The offset of the next `c` from `from`, or the end.
    fn until(&self, c: u8, from: usize) -> usize {
        self.src
            .as_bytes()
            .get(from..)
            .and_then(|rest| rest.iter().position(|&b| b == c))
            .map_or(self.src.len(), |at| from + at)
    }

    /// The end of a run of `\s` from `at`.
    fn spaces(&self, mut at: usize) -> usize {
        while let Some(c) = self.char_at(at).filter(|&c| is_space(u32::from(c))) {
            at += c.len_utf8();
        }
        at
    }

    /// Whether `word` is at `at`, ignoring ASCII case: the end of it.
    fn word(&self, at: usize, word: &str) -> Option<usize> {
        let end = at + word.len();
        self.src
            .get(at..end)
            .filter(|found| found.eq_ignore_ascii_case(word))
            .map(|_| end)
    }

    /// Whether `at` is a word boundary after a word character: the next one is not `\w`.
    fn bound(&self, at: usize) -> bool {
        !self.byte(at).is_some_and(|b| is_word(u32::from(b)))
    }

    /// `word\s*` followed by `c` and `\s*`, as the accessibility rules match.
    fn word_then(&self, word: &str, c: u8) -> Option<usize> {
        let at = self.spaces(self.word(self.p, word)?);
        (self.byte(at) == Some(c)).then(|| self.spaces(at + 1))
    }

    /// `word\s+`, and the byte that must follow when `next` is given.
    fn word_spaces(&self, word: &str, next: Option<u8>) -> Option<usize> {
        let after = self.word(self.p, word)?;
        let at = self.spaces(after);
        if at == after {
            return None;
        }
        match next {
            Some(c) if self.byte(at) == Some(c) => Some(at + 1),
            Some(_) => None,
            None => Some(at),
        }
    }

    /// One of `names` after `word\s+`, followed by `\b`: the end and the index of the name.
    fn word_choice(&self, word: &str, names: &[&str]) -> Option<(usize, usize)> {
        let after = self.word(self.p, word)?;
        let at = self.spaces(after);
        if at == after {
            return None;
        }
        names.iter().enumerate().find_map(|(index, name)| {
            let end = self.word(at, name)?;
            self.bound(end).then_some((end, index))
        })
    }

    /// Reads a keyword statement starting with `c`. Whether one matched.
    fn keyword(&mut self, c: u8) -> bool {
        let lower = c | 32;
        match lower {
            b'a' => {
                if let Some(e) = self.word_then("accTitle", b':') {
                    self.state = S::AccTitle;
                    self.emit(T::AccTitle, self.p, e);
                    return true;
                }
                if let Some(e) = self.word_then("accDescr", b':') {
                    self.state = S::AccDescr;
                    self.emit(T::AccDescr, self.p, e);
                    return true;
                }
                if let Some(e) = self.word_then("accDescr", b'{') {
                    self.state = S::AccDescrMultiline;
                    self.p = e;
                    return true;
                }
            }
            b'h' => {
                if let Some(e) = self.word_spaces("href", Some(b'"')) {
                    self.state = S::Href;
                    self.p = e;
                    return true;
                }
            }
            b'c' => {
                if let Some(e) = self.word_spaces("call", None) {
                    self.state = S::Callbackname;
                    self.p = e;
                    return true;
                }
                if let Some(e) = self.word_spaces("click", None) {
                    self.state = S::Click;
                    self.p = e;
                    return true;
                }
            }
            b'w' => {
                if let Some((e, index)) = self.word_choice("weekday", &DAYS) {
                    let kind = WEEKDAYS.get(index).copied().unwrap_or(T::WeekdayMonday);
                    self.emit(kind, self.p, e);
                    return true;
                }
                if let Some((e, index)) = self.word_choice("weekend", &["friday", "saturday"]) {
                    let kind = if index == 0 {
                        T::WeekendFriday
                    } else {
                        T::WeekendSaturday
                    };
                    self.emit(kind, self.p, e);
                    return true;
                }
            }
            _ => {}
        }

        for &(first, word, tail, kind) in &KEYWORDS {
            if first != lower {
                continue;
            }
            let Some(after) = self.word(self.p, word) else {
                continue;
            };
            let end = match tail {
                Tail::Bound => self.bound(after).then_some(after),
                Tail::Text(stops) => {
                    // One `\s`, then one or more characters other than `stops`.
                    let Some(space) = self.char_at(after).filter(|&c| is_space(u32::from(c)))
                    else {
                        continue;
                    };
                    let from = after + space.len_utf8();
                    let rest = self.src.as_bytes().get(from..).unwrap_or(&[]);
                    let length = rest
                        .iter()
                        .position(|b| stops.contains(b))
                        .unwrap_or(rest.len());
                    (length > 0).then_some(from + length)
                }
            };
            if let Some(end) = end {
                self.emit(kind, self.p, end);
                return true;
            }
        }
        false
    }

    /// A `\d\d\d\d-\d\d-\d\d\b` date at the current position.
    fn date(&self) -> bool {
        let digit = |at: usize| self.byte(at).is_some_and(|b| is_digit(u32::from(b)));
        let p = self.p;
        (0..4).all(|i| digit(p + i))
            && self.byte(p + 4) == Some(b'-')
            && digit(p + 5)
            && digit(p + 6)
            && self.byte(p + 7) == Some(b'-')
            && digit(p + 8)
            && digit(p + 9)
            && self.bound(p + 10)
    }

    /// Reads in the initial state. Whether lexing must stop.
    fn initial(&mut self) -> bool {
        let p = self.p;
        let Some(c) = self.char_at(p) else {
            return false;
        };
        let code = u32::from(c);
        // The UTF-16 unit after `c`: the second half of a surrogate pair is never `%`.
        let d = if c.len_utf16() == 2 {
            None
        } else {
            self.byte(p + c.len_utf8())
        };

        if c == '%' {
            if d != Some(b'%') {
                let mut e = self.until(b'\n', p + 1);
                while self.byte(e) == Some(b'\n') {
                    e += 1;
                }
                self.p = e;
            } else if self.byte(p + 2) == Some(b'{') {
                // The grammar enters a lexer state it never defines, so nothing can follow.
                self.emit(T::OpenDirective, p, p + 3);
                self.emit(T::Error, p, p);
                return true;
            } else {
                self.p = self.until(b'\n', p + 2);
            }
        } else if c != '}' && d == Some(b'%') {
            // Any character but `}` followed by `%` starts a comment that runs to the end of the line.
            self.p = self.until(b'\n', p + c.len_utf8() + 1);
        } else if c == '\n' {
            let mut e = p + 1;
            while self.byte(e) == Some(b'\n') {
                e += 1;
            }
            self.emit(T::Nl, p, e);
        } else if is_space(code) {
            self.p = self.spaces(p);
        } else if u8::try_from(c).is_ok_and(|b| self.keyword(b)) {
            // The keyword rule emitted its token.
        } else if is_digit(code) && self.date() {
            self.emit(T::Date, p, p + 10);
        } else if c != ':' {
            let rest = self.src.as_bytes().get(p..).unwrap_or(&[]);
            let skip = c.len_utf8();
            let e = rest
                .iter()
                .skip(skip)
                .position(|&b| b == b':' || b == b'\n')
                .map_or(self.src.len(), |at| p + skip + at);
            self.emit(T::TaskTxt, p, e);
        } else {
            let rest = self.src.as_bytes().get(p + 1..).unwrap_or(&[]);
            let e = rest
                .iter()
                .position(|&b| b == b'#' || b == b'\n' || b == b';')
                .map_or(self.src.len(), |at| p + 1 + at);
            self.emit(if e > p + 1 { T::TaskData } else { T::Colon }, p, e);
        }
        false
    }
}

/// Splits `src` into tokens, ending with an `End` token.
pub(crate) fn tokenize(src: &str) -> Vec<Token<'_>> {
    let n = src.len();
    let mut lx = Lexer {
        p: 0,
        src,
        state: S::Initial,
        tokens: Vec::new(),
    };

    loop {
        if lx.p >= n {
            let r#type = match lx.state {
                S::AccTitle => T::AccTitleValue,
                S::AccDescr => T::AccDescrValue,
                S::AccDescrMultiline => T::AccDescrMultilineValue,
                S::Href => T::Href,
                S::Callbackname => T::Callbackname,
                S::Callbackargs => T::Callbackargs,
                S::Click => T::Click,
                S::Initial => T::Eof,
            };
            lx.emit(r#type, n, n);
            break;
        }

        let p = lx.p;
        let c = lx.char_at(p).unwrap_or('\0');
        match lx.state {
            S::Initial => {
                if lx.initial() {
                    return lx.tokens;
                }
            }
            S::AccTitle | S::AccDescr => {
                let r#type = if lx.state == S::AccTitle {
                    T::AccTitleValue
                } else {
                    T::AccDescrValue
                };
                lx.state = S::Initial;
                let e = lx.until(b'\n', p);
                lx.emit(r#type, p, e);
            }
            S::AccDescrMultiline => {
                if c == '}' {
                    lx.state = S::Initial;
                    lx.p += 1;
                } else {
                    let e = lx.until(b'}', p);
                    lx.emit(T::AccDescrMultilineValue, p, e);
                }
            }
            S::Href => {
                if c == '"' {
                    lx.state = S::Initial;
                    lx.p += 1;
                } else {
                    let e = lx.until(b'"', p);
                    lx.emit(T::Href, p, e);
                }
            }
            S::Callbackname => {
                if c == '(' {
                    let close = lx.spaces(p + 1);
                    if lx.byte(close) == Some(b')') {
                        lx.state = S::Initial;
                        lx.p = close + 1;
                    } else {
                        lx.state = S::Callbackargs;
                        lx.p += 1;
                    }
                } else {
                    let e = lx.until(b'(', p);
                    lx.emit(T::Callbackname, p, e);
                }
            }
            S::Callbackargs => {
                if c == ')' {
                    lx.state = S::Initial;
                    lx.p += 1;
                } else {
                    let e = lx.until(b')', p);
                    lx.emit(T::Callbackargs, p, e);
                }
            }
            S::Click => {
                if is_space(u32::from(c)) {
                    lx.state = S::Initial;
                    lx.p += c.len_utf8();
                } else {
                    let mut e = p + c.len_utf8();
                    while let Some(ch) = lx.char_at(e).filter(|&ch| !is_space(u32::from(ch))) {
                        e += ch.len_utf8();
                    }
                    lx.emit(T::Click, p, e);
                }
            }
        }
    }

    lx.tokens.push(Token {
        start: n,
        text: "",
        r#type: T::End,
    });
    lx.tokens
}
