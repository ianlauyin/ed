//! The user journey lexer: a port of the rules of Mermaid's journey grammar. Port of Pele's
//! `src/diagrams/journey/lexer.ts`.
//!
//! Offsets are UTF-8. Pele steps over one UTF-16 unit after a keyword's space, which is always
//! one character, so this lexer steps over that character.

use crate::diagrams::common::scan::{Tokens, accessibility, keyword_at, skip_trivia};
use crate::util::chars::{is_space, is_word};

/// The end of the tokens.
pub(crate) const END: u16 = 0;
/// The end of input.
pub(crate) const EOF: u16 = 1;
/// Text no rule matches.
pub(crate) const INVALID: u16 = 2;
/// Line breaks.
pub(crate) const NEWLINE: u16 = 3;
/// `journey`.
pub(crate) const JOURNEY: u16 = 4;
/// A `title` statement.
pub(crate) const TITLE: u16 = 5;
/// `accTitle:`, followed by its value. The accessibility types are numbered consecutively.
pub(crate) const ACC_TITLE: u16 = 6;
/// `accDescr:`, followed by its value.
pub(crate) const ACC_DESCR: u16 = ACC_TITLE + 2;
/// The text of an `accDescr { ... }` block.
pub(crate) const ACC_DESCR_MULTILINE_VALUE: u16 = ACC_TITLE + 4;
/// A section heading.
pub(crate) const SECTION: u16 = 11;
/// A task's name.
pub(crate) const TASK_NAME: u16 = 12;
/// A task's score and actors, from the colon.
pub(crate) const TASK_DATA: u16 = 13;

/// The names the generated parser prints, by token type.
pub(crate) const TOKEN_NAMES: [&str; 15] = [
    "$end",
    "EOF",
    "INVALID",
    "NEWLINE",
    "journey",
    "title",
    "acc_title",
    "acc_title_value",
    "acc_descr",
    "acc_descr_value",
    "acc_descr_multiline_value",
    "section",
    "taskName",
    "taskData",
    ":",
];

/// A lone colon.
const COLON: u16 = 14;

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

/// `#`, a line break, and `;` end every kind of text; a colon also ends a task name or a section.
fn text_end(src: &str, from: usize, colon: bool) -> usize {
    let bytes = src.as_bytes();
    let mut e = from;
    while e < src.len() {
        match bytes.get(e) {
            Some(b'#' | b'\n' | b';') => break,
            Some(b':') if colon => break,
            _ => e += 1,
        }
    }
    e
}

/// Splits preprocessed journey source into tokens.
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
        if keyword_at(src, p, "journey") && !code_at(src, p + 7).is_some_and(is_word) {
            p = emit(&mut out, JOURNEY, p, p + 7);
            continue;
        }
        // `\s` after a keyword also takes a line break, so `title` alone on a line takes the
        // next line as its text.
        if keyword_at(src, p, "title") && space_at(p + 5) {
            let from = p + 5 + len_at(src, p + 5);
            let e = text_end(src, from, false);
            if e > from {
                p = emit(&mut out, TITLE, p, e);
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
            let e = text_end(src, from, true);
            if e > from {
                p = emit(&mut out, SECTION, p, e);
                continue;
            }
        }
        p = match c {
            b':' => {
                let e = text_end(src, p + 1, false);
                if e > p + 1 {
                    emit(&mut out, TASK_DATA, p, e)
                } else {
                    emit(&mut out, COLON, p, p + 1)
                }
            }
            b';' => emit(&mut out, INVALID, p, p + 1),
            _ => {
                let e = text_end(src, p + len_at(src, p), true);
                emit(&mut out, TASK_NAME, p, e)
            }
        };
    }

    // Jison marks its lexer done when a rule runs on empty input, so an accessibility value that
    // matches nothing at the very end of the text is followed by no EOF token.
    if out.starts.last().is_none_or(|&last| last < n) {
        emit(&mut out, EOF, n, n);
    }
    emit(&mut out, END, n, n);
    out
}
