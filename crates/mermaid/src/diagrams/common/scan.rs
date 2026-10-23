//! Lexer rules that Mermaid's journey and timeline grammars have in common, ported from the
//! compiled Jison rules: comments, whitespace, and the `accTitle` and `accDescr` statements.
//! Port of Pele's `src/diagrams/common/scan.ts`.
//!
//! Offsets are UTF-8. Pele reads UTF-16 code units, so a test of the unit after a character
//! sees the second half of a surrogate pair after a supplementary character, which no ASCII test
//! matches.

use crate::util::chars::is_space;

/// Tokens of a Jison-style lexer: parallel lists of types, texts, and UTF-8 starts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tokens<'a> {
    /// Where each token starts.
    pub(crate) starts: Vec<usize>,
    /// The text of each token.
    pub(crate) texts: Vec<&'a str>,
    /// The type of each token.
    pub(crate) types: Vec<u16>,
}

/// Where the line from `from` ends: at the next `\n`, or the end of `src`.
pub(crate) fn line_end(src: &str, from: usize) -> usize {
    src.get(from..).and_then(|rest| rest.find('\n')).map_or(src.len(), |at| from + at)
}

/// Whether `word`, in lowercase ASCII letters, is at `p`. The grammars are case-insensitive.
pub(crate) fn keyword_at(src: &str, p: usize, word: &str) -> bool {
    let bytes = src.as_bytes();
    word.bytes().enumerate().all(|(i, w)| bytes.get(p + i).is_some_and(|&c| c | 32 == w))
}

/// The character at `p`, when `p` is a character boundary inside `src`.
fn char_at(src: &str, p: usize) -> Option<char> {
    src.get(p..)?.chars().next()
}

/// Skips comments and whitespace, and stops at a line break that starts a token, which is a
/// `NEWLINE`. Jison compiles `\%%` to a single `%`, so one percent sign starts a comment, and the
/// second comment rule takes any character before `%%` with it, a line break included.
pub(crate) fn skip_trivia(src: &str, from: usize) -> usize {
    let bytes = src.as_bytes();
    let n = src.len();
    let mut p = from;
    while p < n {
        let Some(c) = char_at(src, p) else {
            return p;
        };
        // The unit after `c`. After a supplementary character it is a low surrogate.
        let next = p + c.len_utf8();
        let after = |k: usize| {
            if c.len_utf16() == 1 { bytes.get(next + k).copied() } else { None }
        };

        if c == '%' && after(0) != Some(b'{') {
            p = line_end(src, p);
        } else if c != '}' && after(0) == Some(b'%') && after(1) == Some(b'%') {
            p = line_end(src, next + 2);
        } else if c == '\n' {
            return p;
        } else if is_space(u32::from(c)) {
            p = next;
            while let Some(c) = char_at(src, p)
                && is_space(u32::from(c))
            {
                p += c.len_utf8();
            }
        } else if c == '#' {
            p = line_end(src, p);
        } else {
            return p;
        }
    }
    p
}

fn skip_space(src: &str, from: usize) -> usize {
    let mut p = from;
    while let Some(c) = char_at(src, p)
        && is_space(u32::from(c))
    {
        p += c.len_utf8();
    }
    p
}

/// Lexes an `accTitle` or `accDescr` statement at `p` into `out` and returns where it ends, or
/// `p` when there is none. The token types `acc_title`, `acc_title_value`, `acc_descr`,
/// `acc_descr_value`, and `acc_descr_multiline_value` are numbered consecutively from `first`.
pub(crate) fn accessibility<'a>(src: &'a str, p: usize, out: &mut Tokens<'a>, first: u16) -> usize {
    let descr = keyword_at(src, p, "accdescr");
    if !descr && !keyword_at(src, p, "acctitle") {
        return p;
    }
    let n = src.len();
    let mut q = skip_space(src, p + 8);
    let c = src.as_bytes().get(q).copied();
    let mut emit = |kind: u16, s: usize, e: usize| {
        out.types.push(kind);
        out.texts.push(src.get(s..e).unwrap_or(""));
        out.starts.push(s);
    };

    if c == Some(b':') {
        q = skip_space(src, q + 1);
        let end = line_end(src, q);
        emit(if descr { first + 2 } else { first }, p, q);
        emit(if descr { first + 3 } else { first + 1 }, q, end);
        return end;
    }
    if !descr || c != Some(b'{') {
        return p;
    }

    q = skip_space(src, q + 1);
    while src.as_bytes().get(q) != Some(&b'}') {
        let end = src.get(q..).and_then(|rest| rest.find('}')).map_or(n, |at| q + at);
        emit(first + 4, q, end);
        if q == n {
            return n;
        }
        q = end;
    }
    q + 1
}
