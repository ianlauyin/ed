//! The XY chart lexer. Port of Pele's `src/diagrams/xychart/lexer.ts`.
//!
//! Pele reads UTF-16 code units. This port reads UTF-8 bytes. Every delimiter is ASCII, a
//! non-ASCII character is decoded whole wherever a rule tests it, and a character outside the
//! Basic Multilingual Plane counts as the two units a rule would see in JavaScript.

use super::tokens::T;
use crate::diagrams::common::caseless::word_at;
use crate::util::chars::{is_digit, is_letter, is_space, is_word};

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
    AxisData,
    AxisBandData,
    Data,
    DataInner,
    String,
    AccTitle,
    AccDescr,
    AccDescrMultiline,
}

impl S {
    /// The token the state gives for the empty rest of the input.
    const fn at_end(self) -> T {
        match self {
            Self::String => T::Str,
            Self::AccTitle => T::AccTitleValue,
            Self::AccDescr => T::AccDescrValue,
            Self::AccDescrMultiline => T::AccDescrMultilineValue,
            _ => T::Eof,
        }
    }
}

/// Mermaid's grammar means to open a markdown string at `"` followed by a backtick, but its
/// quoting turns the rule into one that matches this text and leaves the current state. Markdown
/// strings are never read.
const BROKEN_RULE: &str = concat!(
    "`)",
    "                                    ",
    "{ this.pushstate(md_string); }\n<md_string>(?:(?!`\").)+",
    "                  ",
    "{ return md_str; }\n<md_string>(?:`",
);

/// The token of a one-character ASCII rule.
fn single(c: u32) -> Option<T> {
    Some(match c {
        58 => T::Colon,
        43 => T::Plus,
        44 => T::Comma,
        61 => T::Equals,
        42 => T::Mult,
        35 => T::Brkt,
        95 => T::Underscore,
        46 => T::Dot,
        38 => T::Amp,
        45 => T::Minus,
        59 => T::Semi,
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

/// The end of the run of `\s` at `q`.
fn run(src: &str, mut q: usize) -> usize {
    while let Some(c) = char_at(src, q).filter(|&c| is_space(u32::from(c))) {
        q += c.len_utf8();
    }
    q
}

/// The end of the run of ASCII digits at `q`.
fn digits(src: &str, mut q: usize) -> usize {
    while is_digit(u32::from(byte(src, q))) {
        q += 1;
    }
    q
}

/// Where the next `needle` is from `from`, or the end.
fn up_to(src: &str, from: usize, needle: char) -> usize {
    src.get(from..)
        .and_then(|rest| rest.find(needle))
        .map_or(src.len(), |at| from + at)
}

/// Produces the tokens of Mermaid's Jison lexer for XY charts. An unrecognized character ends the
/// list with an `Error` token. Otherwise the list ends with what the last state gives for the
/// empty rest of the input, then `End`.
#[expect(
    clippy::many_single_char_names,
    reason = "mirrors the names of Pele's lexer"
)]
pub(crate) fn tokenize(src: &str) -> Vec<Token> {
    let n = src.len();
    let mut tokens: Vec<Token> = Vec::new();
    let mut stack: Vec<S> = Vec::new();
    let mut state = S::Initial;
    let mut p = 0;

    let emit = |tokens: &mut Vec<Token>, t: T, start: usize, end: usize| {
        tokens.push(Token { end, start, t });
        end
    };
    let pop = |stack: &mut Vec<S>, state: &mut S| {
        if let Some(previous) = stack.pop() {
            *state = previous;
        }
    };
    let keyword = |p: usize, word: &str| {
        word_at(src, p, word) && !is_word(u32::from(byte(src, p + word.len())))
    };

    while p < n {
        let Some(ch) = char_at(src, p) else {
            break;
        };
        let c = u32::from(ch);
        let width = ch.len_utf8();

        if state == S::String {
            if c == 34 {
                p += 1;
                pop(&mut stack, &mut state);
            } else {
                p = emit(&mut tokens, T::Str, p, up_to(src, p, '"'));
            }
            continue;
        }
        if state == S::AccTitle || state == S::AccDescr {
            p = emit(&mut tokens, state.at_end(), p, up_to(src, p, '\n'));
            pop(&mut stack, &mut state);
            continue;
        }
        if state == S::AccDescrMultiline {
            if c == 125 {
                p += 1;
                pop(&mut stack, &mut state);
            } else {
                p = emit(
                    &mut tokens,
                    T::AccDescrMultilineValue,
                    p,
                    up_to(src, p, '}'),
                );
            }
            continue;
        }

        // The code unit after the first one at `p`: a low surrogate for a character outside the
        // Basic Multilingual Plane.
        let next = if ch.len_utf16() == 2 {
            0xdc00
        } else {
            u32::from(byte(src, p + width))
        };
        if c == 37 && next == 37 {
            p = up_to(src, p + 2, '\n');
            continue;
        }
        // Mermaid's second comment rule takes any character before `%%` with it, a line break included.
        if c != 125 && next == 37 && byte(src, p + width + 1) == b'%' {
            p = up_to(src, p + width + 2, '\n');
            continue;
        }
        if c == 10 || c == 13 {
            // Only a line break that directly follows an axis or a series ends it. One after
            // spaces is skipped with them, and the state carries on to the next line.
            if (state == S::AxisData || state == S::Data) && (c == 10 || next == 10) {
                p = emit(
                    &mut tokens,
                    T::Newline,
                    p,
                    if c == 10 { p + 1 } else { p + 2 },
                );
                pop(&mut stack, &mut state);
                continue;
            }
            let mut e = p + 1;
            while matches!(byte(src, e), b'\n' | b'\r') {
                e += 1;
            }
            p = emit(&mut tokens, T::Newline, p, e);
            continue;
        }

        match c | 32 {
            116 if keyword(p, "title") => {
                p = emit(&mut tokens, T::Title, p, p + 5);
                continue;
            }
            97 if word_at(src, p, "acc") => {
                let descr = word_at(src, p + 3, "descr");
                if descr || word_at(src, p + 3, "title") {
                    let k = run(src, p + 8);
                    let d = byte(src, k);
                    if d == b':' {
                        let t = if descr { T::AccDescr } else { T::AccTitle };
                        p = emit(&mut tokens, t, p, run(src, k + 1));
                        stack.push(state);
                        state = if descr { S::AccDescr } else { S::AccTitle };
                        continue;
                    }
                    if d == b'{' && descr {
                        p = run(src, k + 1);
                        stack.push(state);
                        state = S::AccDescrMultiline;
                        continue;
                    }
                }
            }
            120 => {
                if keyword(p, "xychart-beta") {
                    p = emit(&mut tokens, T::Xychart, p, p + 12);
                    continue;
                }
                if keyword(p, "xychart") {
                    p = emit(&mut tokens, T::Xychart, p, p + 7);
                    continue;
                }
                if keyword(p, "x-axis") {
                    p = emit(&mut tokens, T::XAxis, p, p + 6);
                    stack.push(state);
                    state = S::AxisData;
                    continue;
                }
            }
            121 if keyword(p, "y-axis") => {
                p = emit(&mut tokens, T::YAxis, p, p + 6);
                stack.push(state);
                state = S::AxisData;
                continue;
            }
            118 if word_at(src, p, "vertical") => {
                p = emit(&mut tokens, T::ChartOrientation, p, p + 8);
                continue;
            }
            104 if word_at(src, p, "horizontal") => {
                p = emit(&mut tokens, T::ChartOrientation, p, p + 10);
                continue;
            }
            108 if keyword(p, "line") => {
                p = emit(&mut tokens, T::Line, p, p + 4);
                stack.push(state);
                state = S::Data;
                continue;
            }
            98 if keyword(p, "bar") => {
                p = emit(&mut tokens, T::Bar, p, p + 3);
                stack.push(state);
                state = S::Data;
                continue;
            }
            _ => {}
        }

        if c == 91 {
            if state == S::AxisData {
                stack.push(state);
                state = S::AxisBandData;
            } else if state == S::Data {
                stack.push(state);
                state = S::DataInner;
            }
            p = emit(&mut tokens, T::SquareBracesStart, p, p + 1);
            continue;
        }
        if c == 93 {
            if state == S::DataInner || state == S::AxisBandData {
                pop(&mut stack, &mut state);
            }
            p = emit(&mut tokens, T::SquareBracesEnd, p, p + 1);
            continue;
        }
        if state == S::AxisData && c == 45 && next == 45 && byte(src, p + 2) == b'>' {
            p = emit(&mut tokens, T::ArrowDelimiter, p, p + 3);
            continue;
        }
        if state == S::AxisData || state == S::DataInner {
            let q = if c == 43 || c == 45 { p + 1 } else { p };
            let d = byte(src, q);
            let end = if is_digit(u32::from(d)) {
                let mut e = digits(src, q + 1);
                if byte(src, e) == b'.' && is_digit(u32::from(byte(src, e + 1))) {
                    e = digits(src, e + 2);
                }
                Some(e)
            } else if d == b'.' && is_digit(u32::from(byte(src, q + 1))) {
                Some(digits(src, q + 2))
            } else {
                None
            };
            if let Some(end) = end {
                p = emit(&mut tokens, T::NumberWithDecimal, p, end);
                continue;
            }
        }
        if c == 96 && word_at(src, p, BROKEN_RULE) {
            p += BROKEN_RULE.len();
            pop(&mut stack, &mut state);
            continue;
        }
        if c == 34 {
            p += 1;
            stack.push(state);
            state = S::String;
            continue;
        }
        if is_letter(c) {
            let mut e = p + 1;
            while is_letter(u32::from(byte(src, e))) {
                e += 1;
            }
            p = emit(&mut tokens, T::Alpha, p, e);
            continue;
        }
        if is_digit(c) {
            p = emit(&mut tokens, T::Num, p, digits(src, p + 1));
            continue;
        }
        if is_space(c) {
            p = run(src, p + width);
            continue;
        }
        if let Some(t) = single(c) {
            p = emit(&mut tokens, t, p, p + 1);
            continue;
        }
        break;
    }

    if p < n {
        tokens.push(Token {
            end: p,
            start: p,
            t: T::Error,
        });
        return tokens;
    }
    // Jison tries the rules of the current state once more against the empty rest of the input.
    tokens.push(Token {
        end: n,
        start: n,
        t: state.at_end(),
    });
    tokens.push(Token {
        end: n,
        start: n,
        t: T::End,
    });
    tokens
}
