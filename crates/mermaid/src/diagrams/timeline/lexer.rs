//! The timeline lexer: a port of the rules of Mermaid's timeline grammar. Port of Pele's
//! `src/diagrams/timeline/lexer.ts`.
//!
//! Offsets are UTF-8. Pele steps over one UTF-16 unit after a keyword's space, which is always
//! one character, so this lexer steps over that character.

use crate::diagrams::common::scan::{Tokens, accessibility, keyword_at, line_end, skip_trivia};
use crate::util::chars::{is_space, is_word};

/// The end of the tokens.
pub(crate) const END: u16 = 0;
/// The end of input.
pub(crate) const EOF: u16 = 1;
/// Text no rule matches.
pub(crate) const INVALID: u16 = 2;
/// Line breaks.
pub(crate) const NEWLINE: u16 = 3;
/// `timeline`.
pub(crate) const TIMELINE: u16 = 4;
/// `timeline LR`.
pub(crate) const TIMELINE_LR: u16 = 5;
/// `timeline TD`.
pub(crate) const TIMELINE_TD: u16 = 6;
/// A `title` statement.
pub(crate) const TITLE: u16 = 7;
/// `accTitle:`, followed by its value. The accessibility types are numbered consecutively.
pub(crate) const ACC_TITLE: u16 = 8;
/// A section heading.
pub(crate) const SECTION: u16 = 13;
/// A time period.
pub(crate) const PERIOD: u16 = 14;
/// `: event`.
pub(crate) const EVENT: u16 = 15;

/// `accDescr:`, followed by its value.
pub(crate) const ACC_DESCR: u16 = ACC_TITLE + 2;
/// The text of an `accDescr { ... }` block.
pub(crate) const ACC_DESCR_MULTILINE_VALUE: u16 = ACC_TITLE + 4;

/// The names the generated parser prints, by token type.
pub(crate) const TOKEN_NAMES: [&str; 16] = [
    "$end",
    "EOF",
    "INVALID",
    "NEWLINE",
    "timeline",
    "timeline_lr",
    "timeline_td",
    "title",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "section",
    "period",
    "event",
];

/// The character at `p`, as a code point, when `p` is inside `src`.
fn code_at(src: &str, p: usize) -> Option<u32> {
    src.get(p..)?.chars().next().map(u32::from)
}

/// The length of the character at `p`, or 1 at the end.
fn len_at(src: &str, p: usize) -> usize {
    src.get(p..)
        .and_then(|rest| rest.chars().next())
        .map_or(1, char::len_utf8)
}

/// Splits preprocessed timeline source into tokens.
pub(crate) fn tokenize<'a>(src: &'a str) -> Tokens<'a> {
    let bytes = src.as_bytes();
    let n = src.len();
    let mut out = Tokens::default();
    let mut p = 0;
    let emit = |out: &mut Tokens<'a>, kind: u16, s: usize, e: usize| -> usize {
        out.types.push(kind);
        out.texts.push(src.get(s..e).unwrap_or(""));
        out.starts.push(s);
        e
    };
    let space_at = |at: usize| code_at(src, at).is_some_and(is_space);

    loop {
        p = skip_trivia(src, p);
        if p >= n {
            break;
        }
        let c = bytes.get(p).copied().unwrap_or(0);
        if c == b'\n' {
            let mut e = p + 1;
            while bytes.get(e) == Some(&b'\n') {
                e += 1;
            }
            p = emit(&mut out, NEWLINE, p, e);
            continue;
        }
        if keyword_at(src, p, "timeline") {
            let mut e = p + 8;
            while matches!(bytes.get(e), Some(b' ' | b'\t')) {
                e += 1;
            }
            let lr = keyword_at(src, e, "lr");
            if e > p + 8
                && (lr || keyword_at(src, e, "td"))
                && !code_at(src, e + 2).is_some_and(is_word)
            {
                p = emit(
                    &mut out,
                    if lr { TIMELINE_LR } else { TIMELINE_TD },
                    p,
                    e + 2,
                );
                continue;
            }
            if !code_at(src, p + 8).is_some_and(is_word) {
                p = emit(&mut out, TIMELINE, p, p + 8);
                continue;
            }
        }
        // `\s` after a keyword also takes a line break, so `title` alone on a line takes the
        // next line as its text.
        if keyword_at(src, p, "title") && space_at(p + 5) {
            let after = p + 5 + len_at(src, p + 5);
            if after < n && bytes.get(after) != Some(&b'\n') {
                p = emit(&mut out, TITLE, p, line_end(src, after));
                continue;
            }
        }
        let e = accessibility(src, p, &mut out, ACC_TITLE);
        if e != p {
            p = e;
            continue;
        }
        if keyword_at(src, p, "section") && space_at(p + 7) {
            let from = p + 7 + len_at(src, p + 7);
            let mut e = from;
            while e < n && !matches!(bytes.get(e), Some(b':' | b'\n')) {
                e += 1;
            }
            if e > from {
                p = emit(&mut out, SECTION, p, e);
                continue;
            }
        }
        if c != b':' {
            let mut e = p + len_at(src, p);
            while e < n && !matches!(bytes.get(e), Some(b'#' | b':' | b'\n')) {
                e += 1;
            }
            p = emit(&mut out, PERIOD, p, e);
            continue;
        }
        // An event runs to the end of the line or to a colon that is followed by whitespace.
        if space_at(p + 1) {
            let from = p + 1 + len_at(src, p + 1);
            let mut e = from;
            while e < n {
                match bytes.get(e) {
                    Some(b'\n') => break,
                    Some(b':') if space_at(e + 1) => break,
                    _ => e += 1,
                }
            }
            if e > from {
                p = emit(&mut out, EVENT, p, e);
                continue;
            }
        }
        p = emit(&mut out, INVALID, p, p + 1);
    }

    // Jison marks its lexer done when a rule runs on empty input, so an accessibility value that
    // matches nothing at the very end of the text is followed by no EOF token.
    if out.starts.last().is_none_or(|&last| last < n) {
        emit(&mut out, EOF, n, n);
    }
    emit(&mut out, END, n, n);
    out
}
