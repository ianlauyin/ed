//! The requirement lexer: a state machine equivalent to Mermaid's Jison lexer. Port of Pele's
//! `src/diagrams/requirement/lexer.ts`.
//!
//! Pele reads UTF-16 code units. This port reads UTF-8 bytes: every delimiter and keyword is
//! ASCII, and non-ASCII characters are decoded whole wherever `\s` tests them, so token
//! boundaries match. Each regular expression in Pele is a hand-written matcher below.

use super::tokens::{T, keyword};
use crate::text::label::trim;
use crate::util::chars::{is_line_end, is_space, is_word};

/// A token: its kind, its start as a UTF-8 offset, and its text.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Token<'a> {
    /// The token kind.
    pub(crate) kind: T,
    /// The UTF-8 offset of the token in the source.
    pub(crate) start: usize,
    /// The matched text. An unquoted string is trimmed.
    pub(crate) text: &'a str,
}

/// Lexer start conditions, as in Mermaid's grammar.
#[derive(Clone, Copy, PartialEq, Eq)]
enum S {
    AccDescr,
    AccDescrMultiline,
    AccTitle,
    Initial,
    String,
    Style,
}

/// The longest keyword, in characters.
const LONGEST_KEYWORD: usize = 22;

/// Characters that end an unquoted string: `:,{<>-=` and line breaks.
fn is_stop(c: u8) -> bool {
    matches!(
        c,
        b':' | b',' | b'\r' | b'\n' | b'{' | b'<' | b'>' | b'-' | b'='
    )
}

/// The character at byte `at`, as a code point, or `None` past the end.
fn char_at(src: &str, at: usize) -> Option<u32> {
    src.get(at..)
        .and_then(|rest| rest.chars().next())
        .map(u32::from)
}

/// The end of a run of JavaScript `\s` characters from `at`.
fn skip_spaces(src: &str, at: usize) -> usize {
    let rest = src.get(at..).unwrap_or("");
    at + rest
        .char_indices()
        .find(|&(_, c)| !is_space(u32::from(c)))
        .map_or(rest.len(), |(i, _)| i)
}

/// Whether `src` at `at` spells the ASCII `word` in any case.
fn starts_with_ci(src: &str, at: usize, word: &str) -> bool {
    src.get(at..at + word.len())
        .is_some_and(|text| text.eq_ignore_ascii_case(word))
}

/// `/<word>\s*<sep>\s*/iy` at `at`: the end of the match.
fn acc(src: &str, at: usize, word: &str, sep: u8) -> Option<usize> {
    if !starts_with_ci(src, at, word) {
        return None;
    }
    let q = skip_spaces(src, at + word.len());
    if src.as_bytes().get(q) != Some(&sep) {
        return None;
    }
    Some(skip_spaces(src, q + 1))
}

/// The statements `/direction\s+(?:(TB)|(BT)|(RL)|LR)/gi` matches, scanned left to right:
/// where each kind starts and where its keyword ends.
fn directions(src: &str) -> [(Vec<usize>, Vec<usize>); 4] {
    let mut found: [(Vec<usize>, Vec<usize>); 4] = Default::default();
    let bytes = src.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        let matched = (|| {
            if !starts_with_ci(src, at, "direction") {
                return None;
            }
            let q = skip_spaces(src, at + 9);
            if q == at + 9 {
                return None;
            }
            let pair = src.get(q..q + 2)?;
            let kind = [("TB", 0), ("BT", 1), ("RL", 2), ("LR", 3)]
                .into_iter()
                .find(|(name, _)| pair.eq_ignore_ascii_case(name))?
                .1;
            Some((kind, q + 2))
        })();
        match matched {
            Some((kind, end)) => {
                if let Some((starts, ends)) = found.get_mut(kind) {
                    starts.push(at);
                    ends.push(end);
                }
                at = end;
            }
            None => at += 1,
        }
    }
    found
}

/// Produces the token stream of Mermaid's requirement lexer. Its rules are tried in order and the
/// first match wins, so a line that holds a direction statement anywhere is one token.
#[expect(clippy::too_many_lines, reason = "mirrors Pele's tokenize")]
pub(crate) fn tokenize<'a>(src: &'a str) -> Vec<Token<'a>> {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut tokens: Vec<Token<'a>> = Vec::new();
    let mut stack: Vec<S> = Vec::new();
    let mut state = S::Initial;
    let mut p = 0;

    // Where each kind of direction statement starts and where its keyword ends, found in one pass.
    let dirs = directions(src);
    let any_dir = dirs.iter().any(|(starts, _)| !starts.is_empty());
    let mut dir_seen = [0usize; 4];
    let mut line_end: Option<usize> = None;

    let at = |i: usize| bytes.get(i).copied();
    let line_break = |q: usize| {
        src.get(q..)
            .and_then(|rest| rest.find('\n'))
            .map_or(n, |e| q + e)
    };
    let emit = |tokens: &mut Vec<Token<'a>>, kind: T, s: usize, e: usize| -> usize {
        tokens.push(Token {
            kind,
            start: s,
            text: src.get(s..e).unwrap_or(""),
        });
        e
    };

    while p < n {
        let Some(c) = at(p) else {
            break;
        };
        let ok = match state {
            S::Initial => 'initial: {
                let lower = c | 32;
                if lower == b't'
                    && starts_with_ci(src, p + 1, "itle")
                    && char_at(src, p + 5).is_some_and(is_space)
                {
                    let space = src
                        .get(p + 5..)
                        .and_then(|rest| rest.chars().next())
                        .map_or(1, char::len_utf8);
                    let from = p + 5 + space;
                    let mut q = from;
                    while let Some(d) = at(q) {
                        if d == b'#' || d == b'\n' || d == b';' {
                            break;
                        }
                        q += 1;
                    }
                    if q > from {
                        p = emit(&mut tokens, T::Title, p, q);
                        break 'initial true;
                    }
                }
                if lower == b'a'
                    && at(p + 1).is_some_and(|d| d | 32 == b'c')
                    && at(p + 2).is_some_and(|d| d | 32 == b'c')
                {
                    if let Some(e) = acc(src, p, "accTitle", b':') {
                        stack.push(state);
                        state = S::AccTitle;
                        p = emit(&mut tokens, T::AccTitle, p, e);
                        break 'initial true;
                    }
                    if let Some(e) = acc(src, p, "accDescr", b':') {
                        stack.push(state);
                        state = S::AccDescr;
                        p = emit(&mut tokens, T::AccDescr, p, e);
                        break 'initial true;
                    }
                    if let Some(e) = acc(src, p, "accDescr", b'{') {
                        stack.push(state);
                        state = S::AccDescrMultiline;
                        p = e;
                        break 'initial true;
                    }
                }
                if any_dir && !char_at(src, p).is_some_and(is_line_end) {
                    // `.*direction\s+XX[^\n]*`: the last such statement that starts on this line
                    // decides the token.
                    if line_end.is_none_or(|end| end < p) {
                        let end = src
                            .get(p..)
                            .and_then(|rest| rest.find(|c: char| is_line_end(u32::from(c))))
                            .map_or(n, |e| p + e);
                        line_end = Some(end);
                        for (k, (starts, _)) in dirs.iter().enumerate() {
                            if let Some(seen) = dir_seen.get_mut(k) {
                                while starts.get(*seen).is_some_and(|&s| s <= end) {
                                    *seen += 1;
                                }
                            }
                        }
                    }
                    let found = dirs.iter().enumerate().find_map(|(k, (starts, ends))| {
                        let j = dir_seen.get(k)?.checked_sub(1)?;
                        let start = *starts.get(j)?;
                        (start >= p).then_some((k, *ends.get(j)?))
                    });
                    if let Some((k, end)) = found {
                        p = emit(&mut tokens, T::DirectionTb.offset(k), p, line_break(end));
                        break 'initial true;
                    }
                }

                if c == b'\n' || (c == b'\r' && at(p + 1) == Some(b'\n')) {
                    let mut q = p;
                    loop {
                        match at(q) {
                            Some(b'\n') => q += 1,
                            Some(b'\r') if at(q + 1) == Some(b'\n') => q += 2,
                            _ => break,
                        }
                    }
                    p = emit(&mut tokens, T::Newline, p, q);
                    break 'initial true;
                }
                if char_at(src, p).is_some_and(is_space) {
                    p = skip_spaces(src, p);
                    break 'initial true;
                }
                if c == b'#' || c == b'%' {
                    p = line_break(p);
                    break 'initial true;
                }

                if is_word(u32::from(c)) {
                    let mut q = p + 1;
                    while at(q).is_some_and(|d| is_word(u32::from(d))) {
                        q += 1;
                    }
                    if q - p <= LONGEST_KEYWORD
                        && let Some(word) = src.get(p..q)
                        && let Some(kind) = keyword(&word.to_ascii_lowercase())
                    {
                        if matches!(kind, T::Style | T::Classdef | T::Class) {
                            stack.push(state);
                            state = S::Style;
                        }
                        p = emit(&mut tokens, kind, p, q);
                        break 'initial true;
                    }
                    while at(q).is_some_and(|d| !is_stop(d)) {
                        q += 1;
                    }
                    tokens.push(Token {
                        kind: T::UnqString,
                        start: p,
                        text: trim(src.get(p..q).unwrap_or("")),
                    });
                    p = q;
                    break 'initial true;
                }

                match c {
                    b'{' => p = emit(&mut tokens, T::StructStart, p, p + 1),
                    b'}' => p = emit(&mut tokens, T::StructStop, p, p + 1),
                    b':' => {
                        p = if at(p + 1) == Some(b':') && at(p + 2) == Some(b':') {
                            emit(&mut tokens, T::StyleSeparator, p, p + 3)
                        } else {
                            emit(&mut tokens, T::Colonsep, p, p + 1)
                        };
                    }
                    b'<' => {
                        if at(p + 1) != Some(b'-') {
                            break 'initial false;
                        }
                        p = emit(&mut tokens, T::EndArrowL, p, p + 2);
                    }
                    b'-' => {
                        p = if at(p + 1) == Some(b'>') {
                            emit(&mut tokens, T::EndArrowR, p, p + 2)
                        } else {
                            emit(&mut tokens, T::Line, p, p + 1)
                        };
                    }
                    b'"' => {
                        stack.push(state);
                        state = S::String;
                        p += 1;
                    }
                    b',' => p = emit(&mut tokens, T::Comma, p, p + 1),
                    _ => break 'initial false,
                }
                true
            }
            S::Style => {
                if is_word(u32::from(c)) {
                    let mut q = p + 1;
                    while at(q).is_some_and(|d| is_word(u32::from(d))) {
                        q += 1;
                    }
                    p = emit(&mut tokens, T::Alpha, p, q);
                    true
                } else {
                    let single = match c {
                        b':' => Some(T::Colon),
                        b';' => Some(T::Semicolon),
                        b'%' => Some(T::Percent),
                        b'-' => Some(T::Minus),
                        b'#' => Some(T::Brkt),
                        b',' => Some(T::Comma),
                        _ => None,
                    };
                    if let Some(kind) = single {
                        p = emit(&mut tokens, kind, p, p + 1);
                        true
                    } else {
                        match c {
                            b' ' => {
                                p += 1;
                                true
                            }
                            b'"' => {
                                stack.push(state);
                                state = S::String;
                                p += 1;
                                true
                            }
                            b'\n' => {
                                if let Some(previous) = stack.pop() {
                                    state = previous;
                                }
                                p += 1;
                                true
                            }
                            _ => false,
                        }
                    }
                }
            }
            S::String => {
                if c == b'"' {
                    if let Some(previous) = stack.pop() {
                        state = previous;
                    }
                    p += 1;
                } else {
                    let e = src
                        .get(p..)
                        .and_then(|rest| rest.find('"'))
                        .map_or(n, |e| p + e);
                    p = emit(&mut tokens, T::QString, p, e);
                }
                true
            }
            S::AccTitle | S::AccDescr => {
                let kind = if state == S::AccTitle {
                    T::AccTitleValue
                } else {
                    T::AccDescrValue
                };
                if let Some(previous) = stack.pop() {
                    state = previous;
                }
                p = emit(&mut tokens, kind, p, line_break(p));
                true
            }
            S::AccDescrMultiline => {
                if c == b'}' {
                    if let Some(previous) = stack.pop() {
                        state = previous;
                    }
                    p += 1;
                } else {
                    let e = src
                        .get(p..)
                        .and_then(|rest| rest.find('}'))
                        .map_or(n, |e| p + e);
                    p = emit(&mut tokens, T::AccDescrMultilineValue, p, e);
                }
                true
            }
        };
        if !ok {
            tokens.push(Token {
                kind: T::Error,
                start: p,
                text: "",
            });
            return tokens;
        }
    }

    // At the end of input Jison still tries the rules that can match nothing, once.
    let last = match state {
        S::Initial => Some(T::Eof),
        S::String => Some(T::QString),
        S::AccTitle => Some(T::AccTitleValue),
        S::AccDescr => Some(T::AccDescrValue),
        S::AccDescrMultiline => Some(T::AccDescrMultilineValue),
        S::Style => None,
    };
    if let Some(last) = last {
        emit(&mut tokens, last, n, n);
    }
    emit(&mut tokens, T::End, n, n);
    tokens
}
