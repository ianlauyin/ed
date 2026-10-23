//! The C4 lexer. Port of Pele's `src/diagrams/c4/lexer.ts`.
//!
//! Pele reads UTF-16 code units. This port reads UTF-8 bytes. Every delimiter is ASCII, and a
//! non-ASCII character is decoded whole wherever `\s` looks at it, so token boundaries match.

use super::tokens::T;
use crate::util::chars::{is_line_end, is_space, is_word};

/// A token: its kind and its UTF-8 span. Its text is the source between them.
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
    Struct,
    Attribute,
    String,
    StringKv,
    StringKvKey,
    StringKvValue,
    Acc,
    AccDescrMultiline,
}

/// The `direction` statements, in the order Mermaid's rules try them.
const DIRECTIONS: [(&str, T); 4] = [
    ("TB", T::DirectionTb),
    ("BT", T::DirectionBt),
    ("RL", T::DirectionRl),
    ("LR", T::DirectionLr),
];

/// The character at `at`, or `None` at the end.
fn char_at(src: &str, at: usize) -> Option<char> {
    src.get(at..).and_then(|rest| rest.chars().next())
}

/// The byte at `at`, or 0 at the end.
fn byte(src: &str, at: usize) -> u8 {
    src.as_bytes().get(at).copied().unwrap_or(0)
}

/// The end of the run of `\s` at `q`.
fn skip_ws(src: &str, mut q: usize) -> usize {
    while let Some(c) = char_at(src, q).filter(|&c| is_space(u32::from(c))) {
        q += c.len_utf8();
    }
    q
}

/// Where the next `needle` is from `from`, or the end.
fn up_to(src: &str, from: usize, needle: char) -> usize {
    src.get(from..)
        .and_then(|rest| rest.find(needle))
        .map_or(src.len(), |at| from + at)
}

/// Mermaid's `/.*direction\s+XX[^\n]*/y` at `p`, where `line_end` is the first line terminator
/// from `p`: the end of the match, or `None`. The greedy `.*` takes the last `direction` before
/// the line ends that matches.
fn direction(src: &str, p: usize, line_end: usize, word: &str) -> Option<usize> {
    let line = src.get(p..line_end)?;
    let mut starts: Vec<usize> = line
        .match_indices("direction")
        .map(|(at, _)| p + at)
        .collect();
    while let Some(start) = starts.pop() {
        let gap = start + 9;
        let after = skip_ws(src, gap);
        if after > gap && src.get(after..).is_some_and(|rest| rest.starts_with(word)) {
            return Some(up_to(src, after + word.len(), '\n'));
        }
    }
    None
}

/// What the search for `direction` statements has found so far, so that the rest of the input
/// is searched once, and each line at most once.
#[derive(Default)]
struct Directions {
    /// Whether no `direction` is left.
    done: bool,
    /// Before this offset, no direction statement can start.
    failed: usize,
    /// The first line terminator from where the last search ran.
    line_end: Option<usize>,
    /// Where the next `direction` is, when it was searched for.
    next: Option<usize>,
}

impl Directions {
    /// The direction statement starting at `p`: its token and its end.
    fn at(&mut self, src: &str, p: usize) -> Option<(T, usize)> {
        if self.done || p < self.failed {
            return None;
        }
        if self.next.is_none_or(|next| next < p) {
            self.next = src
                .get(p..)
                .and_then(|rest| rest.find("direction"))
                .map(|at| p + at);
            if self.next.is_none() {
                self.done = true;
                return None;
            }
        }
        let next = self.next?;
        if self.line_end.is_none_or(|end| end < p) {
            let mut end = p;
            while let Some(c) = char_at(src, end).filter(|&c| !is_line_end(u32::from(c))) {
                end += c.len_utf8();
            }
            self.line_end = Some(end);
        }
        let end = self.line_end.unwrap_or(src.len());
        if next >= end {
            return None;
        }
        let found = DIRECTIONS
            .iter()
            .find_map(|&(word, t)| direction(src, p, end, word).map(|stop| (t, stop)));
        if found.is_none() {
            // No direction statement starts anywhere in the rest of this line either.
            self.failed = end;
        }
        found
    }
}

/// Follows the lexer Jison compiles from `c4Diagram.jison`: its rule order, its start
/// conditions, and what it returns once the input runs out in each of them.
#[expect(
    clippy::many_single_char_names,
    reason = "mirrors the names of Pele's lexer"
)]
pub(crate) fn tokenize(src: &str) -> Vec<Token> {
    let n = src.len();
    let mut tokens: Vec<Token> = Vec::new();
    let mut state = S::Initial;
    let mut acc_value = T::AccTitleValue;
    let mut p = 0;
    let mut done = false;
    let mut directions = Directions::default();

    let emit = |tokens: &mut Vec<Token>, t: T, start: usize, end: usize| {
        tokens.push(Token { end, start, t });
        end
    };
    // `word\s[^#\n;]+`
    let statement = |p: usize, word: &str| -> Option<usize> {
        if !src.get(p..).is_some_and(|rest| rest.starts_with(word)) {
            return None;
        }
        let space = char_at(src, p + word.len()).filter(|&c| is_space(u32::from(c)))?;
        let from = p + word.len() + space.len_utf8();
        let mut q = from;
        while q < n && !matches!(byte(src, q), b'#' | b'\n' | b';') {
            q += 1;
        }
        (q > from).then_some(q)
    };

    loop {
        // Jison notes the end of input one call before it reports it, so a rule that matches
        // nothing gets one last turn.
        if done {
            emit(&mut tokens, T::End, n, n);
            break;
        }
        if p >= n {
            done = true;
        }
        let ch = char_at(src, p);
        let c = ch.map_or(u32::MAX, u32::from);

        match state {
            S::Initial => {
                if p >= n {
                    p = emit(&mut tokens, T::Eof, n, n);
                    continue;
                }
                if let Some((t, stop)) = directions.at(src, p) {
                    p = emit(&mut tokens, t, p, stop);
                    continue;
                }
                if c == 116
                    && let Some(end) = statement(p, "title")
                {
                    p = emit(&mut tokens, T::Title, p, end);
                    continue;
                }
                if c == 97 {
                    if let Some(end) = statement(p, "accDescription") {
                        p = emit(&mut tokens, T::AccDescription, p, end);
                        continue;
                    }
                    let rest = src.get(p..).unwrap_or("");
                    let title = rest.starts_with("accTitle");
                    if title || rest.starts_with("accDescr") {
                        let q = skip_ws(src, p + 8);
                        let d = byte(src, q);
                        if d == b':' {
                            let t = if title { T::AccTitle } else { T::AccDescr };
                            p = emit(&mut tokens, t, p, skip_ws(src, q + 1));
                            acc_value = if title {
                                T::AccTitleValue
                            } else {
                                T::AccDescrValue
                            };
                            state = S::Acc;
                            continue;
                        }
                        if d == b'{' && !title {
                            p = skip_ws(src, q + 1);
                            state = S::AccDescrMultiline;
                            continue;
                        }
                    }
                }
                if c == 37 && byte(src, p + 1) == b'%' {
                    // A comment takes the line breaks after it, so no NEWLINE follows.
                    let mut q = up_to(src, p + 2, '\n');
                    while q < n && matches!(byte(src, q), b'\n' | b'\r') {
                        q += 1;
                    }
                    p = q;
                    continue;
                }
                if is_space(c) {
                    let mut q = p;
                    let mut last = None;
                    while let Some(d) = char_at(src, q).filter(|&d| is_space(u32::from(d))) {
                        if d == '\n' {
                            last = Some(q);
                        }
                        q += d.len_utf8();
                    }
                    p = match last {
                        Some(last) => emit(&mut tokens, T::Newline, p, last + 1),
                        None => q,
                    };
                    continue;
                }
                if c == 123 {
                    p = emit(&mut tokens, T::Lbrace, p, p + 1);
                    continue;
                }
                if c == 125 {
                    p = emit(&mut tokens, T::Rbrace, p, p + 1);
                    continue;
                }
                if is_word(c) {
                    let mut q = p + 1;
                    while q < n && is_word(u32::from(byte(src, q))) {
                        q += 1;
                    }
                    let found = if q - p <= 19 {
                        src.get(p..q).and_then(T::keyword)
                    } else {
                        None
                    };
                    if let Some(t) = found {
                        if t > T::C4Deployment {
                            state = S::Struct;
                        }
                        p = emit(&mut tokens, t, p, q);
                        continue;
                    }
                }
                emit(&mut tokens, T::Error, p, p);
                break;
            }
            S::Struct => {
                if p >= n {
                    p = emit(&mut tokens, T::EofInStruct, n, n);
                } else if c == 40 {
                    let mut q = p + 1;
                    while byte(src, q) == b' ' {
                        q += 1;
                    }
                    if byte(src, q) == b',' {
                        p = emit(&mut tokens, T::AttributeEmpty, p, q + 1);
                    } else {
                        p += 1;
                    }
                    state = S::Attribute;
                } else if c == 41 {
                    p += 1;
                    state = S::Initial;
                } else {
                    emit(&mut tokens, T::Error, p, p);
                    break;
                }
            }
            S::Attribute => {
                if p >= n {
                    continue;
                }
                if c == 41 {
                    p += 1;
                    state = S::Initial;
                } else if c == 44 {
                    if byte(src, p + 1) == b',' {
                        p = emit(&mut tokens, T::AttributeEmpty, p, p + 2);
                    } else {
                        p += 1;
                    }
                } else {
                    let mut q = p;
                    while byte(src, q) == b' ' {
                        q += 1;
                    }
                    let d = byte(src, q);
                    if d == b'"' {
                        if byte(src, q + 1) == b'"' {
                            p = emit(&mut tokens, T::AttributeEmpty, p, q + 2);
                        } else {
                            p = q + 1;
                            state = S::String;
                        }
                    } else if d == b'$' {
                        p = q + 1;
                        state = S::StringKv;
                    } else {
                        // An unquoted value runs to the next comma, through closing parentheses
                        // and line breaks.
                        p = emit(&mut tokens, T::Str, p, up_to(src, p, ','));
                    }
                }
            }
            S::String => {
                if c == 34 {
                    p += 1;
                    state = S::Attribute;
                } else {
                    p = emit(&mut tokens, T::Str, p, up_to(src, p, '"'));
                }
            }
            S::StringKv => {
                p = emit(&mut tokens, T::StrKey, p, up_to(src, p, '='));
                state = S::StringKvKey;
            }
            S::StringKvKey => {
                if p >= n {
                    continue;
                }
                let mut q = p + 1;
                if c == 61 {
                    while byte(src, q) == b' ' {
                        q += 1;
                    }
                }
                if c != 61 || byte(src, q) != b'"' {
                    emit(&mut tokens, T::Error, p, p);
                    break;
                }
                p = q + 1;
                state = S::StringKvValue;
            }
            S::StringKvValue => {
                if p >= n {
                    continue;
                }
                if c == 34 {
                    p += 1;
                    state = S::Attribute;
                } else {
                    p = emit(&mut tokens, T::StrValue, p, up_to(src, p, '"'));
                }
            }
            S::Acc => {
                p = emit(&mut tokens, acc_value, p, up_to(src, p, '\n'));
                state = S::Initial;
            }
            S::AccDescrMultiline => {
                if c == 125 {
                    p += 1;
                    state = S::Initial;
                } else {
                    p = emit(
                        &mut tokens,
                        T::AccDescrMultilineValue,
                        p,
                        up_to(src, p, '}'),
                    );
                }
            }
        }
    }

    tokens
}
