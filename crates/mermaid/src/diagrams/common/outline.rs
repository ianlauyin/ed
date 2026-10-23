//! The indented outline grammar that Mermaid's mindmap and kanban diagrams share. Kanban adds
//! `@{ ... }` metadata after a node and keeps `@` out of node ids. Port of Pele's
//! `src/diagrams/common/outline.ts`.
//!
//! Offsets are UTF-8. Every delimiter is ASCII, and the whitespace and line tests decode whole
//! characters, so token boundaries match Pele's UTF-16 ones.

use std::borrow::Cow;

use crate::error::Error;
use crate::util::chars::is_line_end;
use crate::util::chars::is_space;
use crate::util::chars::is_word;

/// Token types. The order is observable: it matches the generated parser's numbering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum T {
    /// The end of the tokens.
    End,
    /// Text no rule matches.
    Error,
    /// A line of only whitespace or a comment.
    Spaceline,
    /// A line break.
    Nl,
    /// The end of input.
    Eof,
    /// `mindmap` or `kanban`.
    Keyword,
    /// The indentation before a node.
    Spacelist,
    /// `::icon(...)` text.
    Icon,
    /// `:::` class text.
    Class,
    /// An opening node delimiter.
    NodeDstart,
    /// A node id.
    NodeId,
    /// Node text.
    NodeDescr,
    /// A closing node delimiter.
    NodeDend,
    /// Kanban `@{ ... }` metadata.
    ShapeData,
}

/// The name the generated parser prints for a token type. It has no name for the end of input
/// and prints its number.
pub(crate) const fn token_name(kind: T, kanban: bool) -> &'static str {
    match kind {
        T::End => "1",
        T::Error => "INVALID",
        T::Spaceline => "SPACELINE",
        T::Nl => "NL",
        T::Eof => "EOF",
        T::Keyword if kanban => "KANBAN",
        T::Keyword => "MINDMAP",
        T::Spacelist => "SPACELIST",
        T::Icon => "ICON",
        T::Class => "CLASS",
        T::NodeDstart => "NODE_DSTART",
        T::NodeId => "NODE_ID",
        T::NodeDescr => "NODE_DESCR",
        T::NodeDend => "NODE_DEND",
        T::ShapeData => "SHAPE_DATA",
    }
}

/// Tokens of an outline: parallel lists of types, texts, and UTF-8 starts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tokens<'a> {
    /// Where each token starts.
    pub(crate) starts: Vec<usize>,
    /// The text of each token. Quoted metadata owns a copy with its line breaks rewritten.
    pub(crate) texts: Vec<Cow<'a, str>>,
    /// The type of each token.
    pub(crate) types: Vec<T>,
}

/// Node shapes, as Mermaid numbers them.
pub(crate) mod node_type {
    /// No shape given.
    pub(crate) const DEFAULT: u8 = 0;
    /// No border; the same as the default.
    pub(crate) const NO_BORDER: u8 = 0;
    /// `(text)`.
    pub(crate) const ROUNDED_RECT: u8 = 1;
    /// `[text]`.
    pub(crate) const RECT: u8 = 2;
    /// `((text))`.
    pub(crate) const CIRCLE: u8 = 3;
    /// `)text(`.
    pub(crate) const CLOUD: u8 = 4;
    /// `))text((`.
    pub(crate) const BANG: u8 = 5;
    /// `{{text}}`.
    pub(crate) const HEXAGON: u8 = 6;
}

/// The node shape for an opening and a closing delimiter. Any opening delimiter pairs with any
/// closing one, as in Mermaid.
pub(crate) fn get_type(start: &str, end: &str) -> u8 {
    match start {
        "[" => node_type::RECT,
        "(" if end == ")" => node_type::ROUNDED_RECT,
        "(" | ")" => node_type::CLOUD,
        "((" => node_type::CIRCLE,
        "))" => node_type::BANG,
        "{{" => node_type::HEXAGON,
        _ => node_type::DEFAULT,
    }
}

/// A decoration for the last node: an icon or a class.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Decoration<'a> {
    /// The class names.
    pub(crate) class: Option<&'a str>,
    /// The icon name.
    pub(crate) icon: Option<&'a str>,
}

/// What the grammar's actions call, as Pele's `OutlineDb` describes it.
pub(crate) trait OutlineDb {
    /// Adds a node at indentation `level` (in UTF-16 units) with its shape `kind` and kanban
    /// metadata.
    fn add_node(&mut self, level: usize, id: &str, descr: &str, kind: u8, shape_data: Option<&str>);

    /// Decorates the last node.
    fn decorate_node(&mut self, decoration: Decoration<'_>);

    /// The node shape for an opening and a closing delimiter.
    fn get_type(&self, start: &str, end: &str) -> u8 {
        get_type(start, end)
    }
}

/// Lexer start conditions.
#[derive(Clone, Copy, PartialEq, Eq)]
enum S {
    Class,
    Data,
    DataStr,
    Icon,
    Initial,
    Node,
    Nstr,
    Nstr2,
}

/// The character at `p`, when `p` is a character boundary inside `src`.
fn char_at(src: &str, p: usize) -> Option<char> {
    src.get(p..)?.chars().next()
}

/// `\s+` from `p`, or `None` for an empty run.
fn spaces(src: &str, p: usize) -> Option<usize> {
    let mut at = p;
    while let Some(c) = char_at(src, at)
        && is_space(u32::from(c))
    {
        at += c.len_utf8();
    }
    (at > p).then_some(at)
}

/// `.+` from `p`: to the first line terminator, or `None` for an empty run.
fn line(src: &str, p: usize) -> Option<usize> {
    let rest = src.get(p..)?;
    let len = rest.find(|c: char| is_line_end(u32::from(c))).unwrap_or(rest.len());
    (len > 0).then_some(p + len)
}

/// A run of bytes outside `stops` from `p`, or `None` for an empty run. Every stop is ASCII.
fn run(src: &str, p: usize, stops: &[u8]) -> Option<usize> {
    let len = src.as_bytes().get(p..)?.iter().take_while(|b| !stops.contains(b)).count();
    (len > 0).then_some(p + len)
}

/// Jison's case-insensitive flag folds ASCII letters only.
fn letters(src: &str, p: usize, word: &str) -> bool {
    let bytes = src.as_bytes();
    word.bytes().enumerate().all(|(k, w)| bytes.get(p + k).is_some_and(|&c| c | 32 == w))
}

/// `text.replace(/\n\s*/g, '<br/>')`.
fn breaks(text: &str) -> Cow<'_, str> {
    if !text.contains('\n') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 8);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\n' {
            out.push(c);
            continue;
        }
        out.push_str("<br/>");
        while chars.next_if(|&c| is_space(u32::from(c))).is_some() {}
    }
    Cow::Owned(out)
}

/// Appends a token.
fn put<'a>(out: &mut Tokens<'a>, kind: T, start: usize, text: Cow<'a, str>) {
    out.types.push(kind);
    out.texts.push(text);
    out.starts.push(start);
}

/// Splits an outline into tokens. The list ends with `EOF` and `END`, or with `ERROR` where no
/// rule matches.
#[expect(clippy::many_single_char_names, reason = "names mirror Pele's outline.ts for side-by-side review")]
pub(crate) fn tokenize<'a>(src: &'a str, kanban: bool) -> Tokens<'a> {
    let bytes = src.as_bytes();
    let n = src.len();
    let mut out = Tokens::default();
    let keyword = if kanban { "kanban" } else { "mindmap" };
    let id_stops: &[u8] = if kanban { b"([\n){}@" } else { b"([\n){}" };
    let mut state = S::Initial;
    let mut p = 0;
    let mut next_break: Option<usize> = None;

    while p < n {
        let byte = |at: usize| bytes.get(at).copied();
        let c = byte(p);
        let d = byte(p + 1);
        // `(end, next state)` for a token from `p` to `end`.
        let emit = |out: &mut Tokens<'a>, kind: T, end: usize, next: S| {
            put(out, kind, p, Cow::Borrowed(src.get(p..end).unwrap_or("")));
            (end, next)
        };

        let step = match state {
            S::Initial => {
                let w = spaces(src, p).unwrap_or(p);
                if kanban && c == Some(b'@') && d == Some(b'{') {
                    put(&mut out, T::ShapeData, p, Cow::Borrowed(""));
                    Some((p + 2, S::Data))
                } else if byte(w) == Some(b'%') && byte(w + 1) == Some(b'%') {
                    Some(emit(&mut out, T::Spaceline, line(src, w).unwrap_or(w), S::Initial))
                } else if w > p {
                    if next_break.is_none_or(|at| at < p) {
                        next_break = Some(src.get(p..).and_then(|rest| rest.find('\n')).map_or(n, |at| p + at));
                    }
                    let last_break = src.get(..w).and_then(|head| head.rfind('\n'));
                    if next_break.is_some_and(|at| at >= w) {
                        Some(emit(&mut out, T::Spacelist, w, S::Initial))
                    } else if let Some(e) = last_break.filter(|&e| e > p) {
                        Some(emit(&mut out, T::Spaceline, e + 1, S::Initial))
                    } else {
                        Some(emit(&mut out, T::Nl, p + 1, S::Initial))
                    }
                } else if letters(src, p, keyword) && !byte(p + keyword.len()).is_some_and(|b| is_word(u32::from(b))) {
                    Some(emit(&mut out, T::Keyword, p + keyword.len(), S::Initial))
                } else if c == Some(b':') && d == Some(b':') && byte(p + 2) == Some(b':') {
                    Some((p + 3, S::Class))
                } else if c == Some(b':') && d == Some(b':') && letters(src, p + 2, "icon") && byte(p + 6) == Some(b'(')
                {
                    Some((p + 7, S::Icon))
                } else if (c == Some(b'-') && d == Some(b')'))
                    || (c == Some(b'(') && (d == Some(b'-') || d == Some(b'(')))
                    || ((c == Some(b')') || c == Some(b'{')) && d == c)
                {
                    Some(emit(&mut out, T::NodeDstart, p + 2, S::Node))
                } else if matches!(c, Some(b')' | b'(' | b'[')) {
                    Some(emit(&mut out, T::NodeDstart, p + 1, S::Node))
                } else {
                    run(src, p, id_stops).map(|e| emit(&mut out, T::NodeId, e, S::Initial))
                }
            }
            S::Node => match c {
                Some(b'"') => Some(if d == Some(b'`') { (p + 2, S::Nstr2) } else { (p + 1, S::Nstr) }),
                Some(b')' | b'(') => {
                    let end = if d == c || (c == Some(b'(') && d == Some(b'-')) { p + 2 } else { p + 1 };
                    Some(emit(&mut out, T::NodeDend, end, S::Initial))
                }
                Some(b']') => Some(emit(&mut out, T::NodeDend, p + 1, S::Initial)),
                _ if (c == Some(b'}') && d == Some(b'}')) || (c == Some(b'-') && d == Some(b')')) => {
                    Some(emit(&mut out, T::NodeDend, p + 2, S::Initial))
                }
                // A lone closing brace takes the rest of its line as text.
                Some(b'}') => line(src, p).map(|e| emit(&mut out, T::NodeDescr, e, S::Node)),
                _ => run(src, p, b")](}").map(|e| emit(&mut out, T::NodeDescr, e, S::Node)),
            },
            S::Nstr => {
                if c == Some(b'"') {
                    Some((p + 1, S::Node))
                } else {
                    let e = src.get(p..).and_then(|rest| rest.find('"')).map_or(n, |at| p + at);
                    Some(emit(&mut out, T::NodeDescr, e, S::Nstr))
                }
            }
            S::Nstr2 => {
                if c == Some(b'`') && d == Some(b'"') {
                    Some((p + 2, S::Node))
                } else {
                    run(src, p, b"`\"").map(|e| emit(&mut out, T::NodeDescr, e, S::Nstr2))
                }
            }
            S::Class => {
                if let Some(e) = line(src, p) {
                    Some(emit(&mut out, T::Class, e, S::Initial))
                } else if c == Some(b'\n') {
                    Some((p + 1, S::Initial))
                } else {
                    None
                }
            }
            S::Icon => {
                if c == Some(b')') {
                    Some((p + 1, S::Initial))
                } else {
                    let e = src.get(p..).and_then(|rest| rest.find(')')).map_or(n, |at| p + at);
                    Some(emit(&mut out, T::Icon, e, S::Icon))
                }
            }
            S::Data => match c {
                Some(b'"') => Some(emit(&mut out, T::ShapeData, p + 1, S::DataStr)),
                Some(b'}') => Some((p + 1, S::Initial)),
                _ => run(src, p, b"}^\"").map(|e| emit(&mut out, T::ShapeData, e, S::Data)),
            },
            S::DataStr => {
                if c == Some(b'"') {
                    Some(emit(&mut out, T::ShapeData, p + 1, S::Data))
                } else {
                    let e = src.get(p..).and_then(|rest| rest.find('"')).map_or(n, |at| p + at);
                    put(&mut out, T::ShapeData, p, breaks(src.get(p..e).unwrap_or("")));
                    Some((e, S::DataStr))
                }
            }
        };

        let Some((end, next)) = step else {
            break;
        };
        p = end;
        state = next;
    }

    if p < n {
        put(&mut out, T::Error, p, Cow::Borrowed(""));
    } else {
        if state == S::Initial {
            put(&mut out, T::Eof, n, Cow::Borrowed(""));
        }
        put(&mut out, T::End, n, Cow::Borrowed(""));
    }
    out
}

const STOP: &str = "'SPACELINE', 'NL', 'EOF'";

/// Token types that can start a statement after the first.
const fn starts_statement(kind: T) -> bool {
    matches!(kind, T::Spaceline | T::Spacelist | T::Icon | T::Class | T::NodeDstart | T::NodeId)
}

const fn is_stop(kind: T) -> bool {
    matches!(kind, T::Spaceline | T::Nl | T::Eof)
}

/// Parses an outline, making the calls on `db` that the grammar's actions make, in the same
/// order.
///
/// # Errors
///
/// Returns the syntax error the generated parser reports, in the state where it would.
pub(crate) fn parse_outline(src: &str, db: &mut impl OutlineDb, kanban: bool) -> Result<(), Error> {
    let diagram = if kanban { "kanban" } else { "mindmap" };
    let tokens = tokenize(src, kanban);
    let kind = |i: usize| tokens.types.get(i).copied().unwrap_or(T::End);
    let text = |i: usize| tokens.texts.get(i).map_or("", |text| text.as_ref());
    let data = if kanban { ", 'SHAPE_DATA'" } else { "" };
    let mut i = 0;

    let fail = |i: usize, expected: &str| -> Error {
        let start = tokens.starts.get(i).copied().unwrap_or(src.len());
        let t = kind(i);
        if t == T::Error {
            return Error::syntax(diagram, src, start, "", true);
        }
        let mut detail = String::from("Expecting ");
        detail.push_str(expected);
        detail.push_str(", got '");
        detail.push_str(token_name(t, kanban));
        detail.push('\'');
        Error::syntax(diagram, src, start, &detail, false)
    };
    let expected = |parts: &[&str]| parts.concat();

    let keyword = expected(&["'", token_name(T::Keyword, kanban), "'"]);
    if kind(i) == T::Spaceline {
        i += 1;
        while matches!(kind(i), T::Spaceline | T::Nl) {
            i += 1;
        }
        if kind(i) != T::Keyword {
            return Err(fail(i, &expected(&["'SPACELINE', 'NL', ", &keyword])));
        }
    } else if kind(i) != T::Keyword {
        return Err(fail(i, &expected(&["'SPACELINE', ", &keyword])));
    }
    i += 1;
    let mut first = "'SPACELINE', 'NL', 'SPACELIST', 'ICON', 'CLASS', 'NODE_DSTART', 'NODE_ID'";
    if kind(i) == T::Nl {
        i += 1;
        first = "'SPACELINE', 'SPACELIST', 'ICON', 'CLASS', 'NODE_DSTART', 'NODE_ID'";
    }

    loop {
        let mut t = kind(i);
        let mut level = 0;
        let indented = t == T::Spacelist;
        if indented {
            level = text(i).encode_utf16().count();
            i += 1;
            t = kind(i);
        }

        if t == T::NodeId || t == T::NodeDstart {
            let mut id = None;
            let mut node_kind = 0;
            if t == T::NodeId {
                id = Some(text(i));
                i += 1;
            }
            let descr = if kind(i) == T::NodeDstart {
                let start = text(i);
                i += 1;
                if kind(i) != T::NodeDescr {
                    return Err(fail(i, "'NODE_DESCR'"));
                }
                let descr = text(i);
                i += 1;
                if kind(i) != T::NodeDend {
                    return Err(fail(i, "'NODE_DEND'"));
                }
                let end = text(i);
                i += 1;
                if !is_stop(kind(i)) && kind(i) != T::ShapeData {
                    return Err(fail(i, &expected(&[STOP, data])));
                }
                node_kind = db.get_type(start, end);
                descr
            } else {
                if !is_stop(kind(i)) && kind(i) != T::ShapeData {
                    return Err(fail(i, &expected(&[STOP, ", 'NODE_DSTART'", data])));
                }
                id.unwrap_or("")
            };

            if kind(i) == T::ShapeData {
                let mut shape_data = String::from(text(i));
                i += 1;
                while kind(i) == T::ShapeData {
                    shape_data.push_str(text(i));
                    i += 1;
                }
                if !is_stop(kind(i)) {
                    return Err(fail(i, &expected(&[STOP, data])));
                }
                db.add_node(level, id.unwrap_or(descr), descr, node_kind, Some(&shape_data));
            } else {
                db.add_node(level, id.unwrap_or(descr), descr, node_kind, None);
            }
        } else if t == T::Icon || t == T::Class {
            let value = text(i);
            i += 1;
            if !is_stop(kind(i)) {
                return Err(fail(i, STOP));
            }
            db.decorate_node(if t == T::Icon {
                Decoration { class: None, icon: Some(value) }
            } else {
                Decoration { class: Some(value), icon: None }
            });
        } else if indented {
            if !is_stop(t) {
                return Err(fail(i, &expected(&[STOP, ", 'ICON', 'CLASS', 'NODE_DSTART', 'NODE_ID'"])));
            }
        } else if t == T::Spaceline {
            i += 1;
            if !is_stop(kind(i)) {
                return Err(fail(i, STOP));
            }
        } else {
            return Err(fail(i, first));
        }

        i += 1;
        while matches!(kind(i), T::Nl | T::Eof) {
            i += 1;
        }
        if kind(i) == T::End {
            return Ok(());
        }
        if !starts_statement(kind(i)) {
            return Err(fail(i, &expected(&[STOP, ", 'SPACELIST', 'ICON', 'CLASS', 'NODE_DSTART', 'NODE_ID'"])));
        }
    }
}
