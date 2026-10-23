//! The indented outline grammar that Mermaid's mindmap and kanban diagrams share. Kanban adds
//! `@{ ... }` metadata after a node and keeps `@` out of node ids. Port of Pele's
//! `src/diagrams/common/outline.ts`.
//!
//! Offsets are UTF-8. Every delimiter is ASCII, and the whitespace and line tests decode whole
//! characters, so token boundaries match Pele's UTF-16 ones.

use std::borrow::Cow;

use crate::error::Error;
use crate::util::chars::{is_line_end, is_space, is_word};

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
pub(crate) fn token_name(kind: T, kanban: bool) -> &'static str {
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
    let len = rest
        .find(|c: char| is_line_end(u32::from(c)))
        .unwrap_or(rest.len());
    (len > 0).then_some(p + len)
}

/// A run of bytes outside `stops` from `p`, or `None` for an empty run. Every stop is ASCII.
fn run(src: &str, p: usize, stops: &[u8]) -> Option<usize> {
    let len = src
        .as_bytes()
        .get(p..)?
        .iter()
        .take_while(|b| !stops.contains(b))
        .count();
    (len > 0).then_some(p + len)
}

/// Jison's case-insensitive flag folds ASCII letters only.
fn letters(src: &str, p: usize, word: &str) -> bool {
    let bytes = src.as_bytes();
    word.bytes()
        .enumerate()
        .all(|(k, w)| bytes.get(p + k).is_some_and(|&c| c | 32 == w))
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
#[expect(
    clippy::many_single_char_names,
    reason = "names mirror Pele's outline.ts for side-by-side review"
)]
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
                    Some(emit(
                        &mut out,
                        T::Spaceline,
                        line(src, w).unwrap_or(w),
                        S::Initial,
                    ))
                } else if w > p {
                    if next_break.is_none_or(|at| at < p) {
                        next_break = Some(
                            src.get(p..)
                                .and_then(|rest| rest.find('\n'))
                                .map_or(n, |at| p + at),
                        );
                    }
                    let last_break = src.get(..w).and_then(|head| head.rfind('\n'));
                    if next_break.is_some_and(|at| at >= w) {
                        Some(emit(&mut out, T::Spacelist, w, S::Initial))
                    } else if let Some(e) = last_break.filter(|&e| e > p) {
                        Some(emit(&mut out, T::Spaceline, e + 1, S::Initial))
                    } else {
                        Some(emit(&mut out, T::Nl, p + 1, S::Initial))
                    }
                } else if letters(src, p, keyword)
                    && !byte(p + keyword.len()).is_some_and(|b| is_word(u32::from(b)))
                {
                    Some(emit(&mut out, T::Keyword, p + keyword.len(), S::Initial))
                } else if c == Some(b':') && d == Some(b':') && byte(p + 2) == Some(b':') {
                    Some((p + 3, S::Class))
                } else if c == Some(b':')
                    && d == Some(b':')
                    && letters(src, p + 2, "icon")
                    && byte(p + 6) == Some(b'(')
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
                Some(b'"') => Some(if d == Some(b'`') {
                    (p + 2, S::Nstr2)
                } else {
                    (p + 1, S::Nstr)
                }),
                Some(b')' | b'(') => {
                    let end = if d == c || (c == Some(b'(') && d == Some(b'-')) {
                        p + 2
                    } else {
                        p + 1
                    };
                    Some(emit(&mut out, T::NodeDend, end, S::Initial))
                }
                Some(b']') => Some(emit(&mut out, T::NodeDend, p + 1, S::Initial)),
                _ if (c == Some(b'}') && d == Some(b'}'))
                    || (c == Some(b'-') && d == Some(b')')) =>
                {
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
                    let e = src
                        .get(p..)
                        .and_then(|rest| rest.find('"'))
                        .map_or(n, |at| p + at);
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
                    let e = src
                        .get(p..)
                        .and_then(|rest| rest.find(')'))
                        .map_or(n, |at| p + at);
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
                    let e = src
                        .get(p..)
                        .and_then(|rest| rest.find('"'))
                        .map_or(n, |at| p + at);
                    put(
                        &mut out,
                        T::ShapeData,
                        p,
                        breaks(src.get(p..e).unwrap_or("")),
                    );
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
fn starts_statement(kind: T) -> bool {
    matches!(
        kind,
        T::Spaceline | T::Spacelist | T::Icon | T::Class | T::NodeDstart | T::NodeId
    )
}

fn is_stop(kind: T) -> bool {
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
                db.add_node(
                    level,
                    id.unwrap_or(descr),
                    descr,
                    node_kind,
                    Some(&shape_data),
                );
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
                Decoration {
                    class: None,
                    icon: Some(value),
                }
            } else {
                Decoration {
                    class: Some(value),
                    icon: None,
                }
            });
        } else if indented {
            if !is_stop(t) {
                return Err(fail(
                    i,
                    &expected(&[STOP, ", 'ICON', 'CLASS', 'NODE_DSTART', 'NODE_ID'"]),
                ));
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
            return Err(fail(
                i,
                &expected(&[
                    STOP,
                    ", 'SPACELIST', 'ICON', 'CLASS', 'NODE_DSTART', 'NODE_ID'",
                ]),
            ));
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::unreadable_literal,
    reason = "hashes as the Pele scripts print them"
)]
mod tests {
    //! Checks against Pele (`obsidianmd/pele` at `458322a`). Every expected value comes from
    //! running Pele's `tokenize` and `parseOutline` with `bun`, recording the database calls,
    //! never from this port. Corpus results compare FNV-1a hashes of the same serialization.

    use super::*;
    use crate::diagrams::common::tokens::tests::{CORPUS, error, fnv, inputs, utf16};

    #[derive(Default)]
    struct Recorder(Vec<String>);

    impl OutlineDb for Recorder {
        fn add_node(
            &mut self,
            level: usize,
            id: &str,
            descr: &str,
            kind: u8,
            shape_data: Option<&str>,
        ) {
            self.0.push(format!(
                "add|{level}|{id}|{descr}|{kind}|{}",
                shape_data.unwrap_or("~")
            ));
        }

        fn decorate_node(&mut self, decoration: Decoration<'_>) {
            self.0.push(match decoration.icon {
                Some(icon) => format!("icon|{icon}"),
                None => format!("class|{}", decoration.class.unwrap_or("")),
            });
        }
    }

    fn tokens(src: &str, kanban: bool) -> String {
        let t = tokenize(src, kanban);
        t.types
            .iter()
            .zip(&t.starts)
            .zip(&t.texts)
            .map(|((&kind, &start), text)| format!("{}@{}:{text}", kind as u8, utf16(src, start)))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn parsed(src: &str, kanban: bool) -> String {
        let mut db = Recorder::default();
        match parse_outline(src, &mut db, kanban) {
            Ok(()) => db.0.join("\n"),
            Err(e) => db.0.join("\n") + "\n" + &error(&e),
        }
    }

    #[test]
    fn parse_outline_matches_pele_on_edge_cases() {
        let cases: &[(&str, &str, &str)] = &[
            (
                "mindmap\n  root((Root))\n    A[Square]\n    B(Rounded)\n      C)Cloud(\n      D))Bang((\n      E{{Hex}}\n      F(-Cloud-)\n      G\n",
                "5@0:mindmap\n3@7:\n\n6@8:  \n10@10:root\n9@14:((\n11@16:Root\n12@20:))\n3@22:\n\n6@23:    \n10@27:A\n9@28:[\n11@29:Square\n12@35:]\n3@36:\n\n6@37:    \n10@41:B\n9@42:(\n11@43:Rounded\n12@50:)\n3@51:\n\n6@52:      \n10@58:C\n9@59:)\n11@60:Cloud\n12@65:(\n3@66:\n\n6@67:      \n10@73:D\n9@74:))\n11@76:Bang\n12@80:((\n3@82:\n\n6@83:      \n10@89:E\n9@90:{{\n11@92:Hex\n12@95:}}\n3@97:\n\n6@98:      \n10@104:F\n9@105:(-\n11@107:Cloud-\n12@113:)\n3@114:\n\n6@115:      \n10@121:G\n3@122:\n\n4@123:\n0@123:",
                "add|2|root|Root|3|~\nadd|4|A|Square|2|~\nadd|4|B|Rounded|1|~\nadd|6|C|Cloud|4|~\nadd|6|D|Bang|5|~\nadd|6|E|Hex|6|~\nadd|6|F|Cloud-|0|~\nadd|6|G|G|0|~",
            ),
            (
                "mindmap\nroot\n  child\n  ::icon(fa fa-book)\n  :::urgent large\n  other\n",
                "5@0:mindmap\n3@7:\n\n10@8:root\n3@12:\n\n6@13:  \n10@15:child\n3@20:\n\n6@21:  \n7@30:fa fa-book\n3@41:\n\n6@42:  \n8@47:urgent large\n3@59:\n\n6@60:  \n10@62:other\n3@67:\n\n4@68:\n0@68:",
                "add|0|root|root|0|~\nadd|2|child|child|0|~\nicon|fa fa-book\nclass|urgent large\nadd|2|other|other|0|~",
            ),
            (
                "  %% comment\n\nmindmap\n  a[\"quoted ) text\"]\n  b[\"`**md** text`\"]\n",
                "2@0:  %% comment\n2@12:\n\n\n5@14:mindmap\n3@21:\n\n6@22:  \n10@24:a\n9@25:[\n11@27:quoted ) text\n12@41:]\n3@42:\n\n6@43:  \n10@45:b\n9@46:[\n11@49:**md** text\n12@62:]\n3@63:\n\n4@64:\n0@64:",
                "add|2|a|quoted ) text|2|~\nadd|2|b|**md** text|2|~",
            ),
            (
                "MindMap\n\troot\n\t\tchild\n",
                "5@0:MindMap\n3@7:\n\n6@8:\t\n10@9:root\n3@13:\n\n6@14:\t\t\n10@16:child\n3@21:\n\n4@22:\n0@22:",
                "add|1|root|root|0|~\nadd|2|child|child|0|~",
            ),
            (
                "mindmap\n  a[unclosed\n",
                "5@0:mindmap\n3@7:\n\n6@8:  \n10@10:a\n9@11:[\n11@12:unclosed\n\n0@21:",
                "\nE3:1:Parse error on line 3:\n\n^\nExpecting 'NODE_DEND', got '1'",
            ),
            (
                "mindmap\n  a}\n",
                "5@0:mindmap\n3@7:\n\n6@8:  \n10@10:a\n1@11:",
                "\nE2:4:Lexical error on line 2. Unrecognized text.\n  a}\n---^",
            ),
            (
                "mindmap\n  a[x}y]\n",
                "5@0:mindmap\n3@7:\n\n6@8:  \n10@10:a\n9@11:[\n11@12:x\n11@13:}y]\n11@16:\n\n0@17:",
                "\nE2:6:Parse error on line 2:\n  a[x}y]\n-----^\nExpecting 'NODE_DEND', got 'NODE_DESCR'",
            ),
            (
                "mindmap\n  a(\n",
                "5@0:mindmap\n3@7:\n\n6@8:  \n10@10:a\n9@11:(\n11@12:\n\n0@13:",
                "\nE3:1:Parse error on line 3:\n\n^\nExpecting 'NODE_DEND', got '1'",
            ),
            (
                "mindmap root",
                "5@0:mindmap\n6@7: \n10@8:root\n4@12:\n0@12:",
                "add|1|root|root|0|~",
            ),
            (
                "",
                "4@0:\n0@0:",
                "\nE1:1:Parse error on line 1:\n\n^\nExpecting 'SPACELINE', 'MINDMAP', got 'EOF'",
            ),
            (
                "mindmap\n\n\n   \n  root\n   \n",
                "5@0:mindmap\n2@7:\n\n\n   \n\n6@14:  \n10@16:root\n2@20:\n   \n\n4@25:\n0@25:",
                "\nE5:1:Parse error on line 5:\n  root\n^\nExpecting 'SPACELINE', 'NL', 'EOF', got 'SPACELIST'",
            ),
            (
                "mindmap\n  root\u{3000}child\n  \u{3000}\u{3000}x\n",
                "5@0:mindmap\n3@7:\n\n6@8:  \n10@10:root\u{3000}child\n3@20:\n\n6@21:  \u{3000}\u{3000}\n10@25:x\n3@26:\n\n4@27:\n0@27:",
                "add|2|root\u{3000}child|root\u{3000}child|0|~\nadd|4|x|x|0|~",
            ),
            (
                "mindmap\n  :::a\u{2028}b\n",
                "5@0:mindmap\n3@7:\n\n6@8:  \n8@13:a\n6@14:\u{2028}\n10@15:b\n3@16:\n\n4@17:\n0@17:",
                "\nE2:7:Parse error on line 2:\n  :::a\u{2028}b\n------^\nExpecting 'SPACELINE', 'NL', 'EOF', got 'SPACELIST'",
            ),
            (
                "mindmap\n  :::\n  x\n",
                "5@0:mindmap\n3@7:\n\n6@8:  \n6@14:  \n10@16:x\n3@17:\n\n4@18:\n0@18:",
                "\nE3:1:Parse error on line 3:\n  x\n^\nExpecting 'SPACELINE', 'NL', 'EOF', 'ICON', 'CLASS', 'NODE_DSTART', 'NODE_ID', got 'SPACELIST'",
            ),
            (
                "mindmap\n  ::icon(\n",
                "5@0:mindmap\n3@7:\n\n6@8:  \n7@17:\n\n0@18:",
                "\nE3:1:Parse error on line 3:\n\n^\nExpecting 'SPACELINE', 'NL', 'EOF', got '1'",
            ),
            (
                "kanban\n  todo[Todo]\n    t1[Task]@{ ticket: MC-1, assigned: 'me' }\n    t2@{ label: \"multi\n    line\n  text\" }\n",
                "5@0:kanban\n3@6:\n\n6@7:  \n10@9:todo\n9@13:[\n11@14:Todo\n12@18:]\n3@19:\n\n6@20:    \n10@24:t1\n9@26:[\n11@27:Task\n12@31:]\n13@32:\n13@34: ticket: MC-1, assigned: 'me' \n3@65:\n\n6@66:    \n10@70:t2\n13@72:\n13@74: label: \n13@82:\"\n13@83:multi<br/>line<br/>text\n13@104:\"\n13@105: \n3@107:\n\n4@108:\n0@108:",
                "add|2|todo|Todo|2|~\nadd|4|t1|Task|2| ticket: MC-1, assigned: 'me' \nadd|4|t2|t2|0| label: \"multi<br/>line<br/>text\" ",
            ),
            (
                "kanban\n  a@b\n",
                "5@0:kanban\n3@6:\n\n6@7:  \n10@9:a\n1@10:",
                "\nE2:4:Lexical error on line 2. Unrecognized text.\n  a@b\n---^",
            ),
            (
                "kanban\n  t[x]@{ a: ^ }\n",
                "5@0:kanban\n3@6:\n\n6@7:  \n10@9:t\n9@10:[\n11@11:x\n12@12:]\n13@13:\n13@15: a: \n1@19:",
                "\nE2:13:Lexical error on line 2. Unrecognized text.\n  t[x]@{ a: ^ }\n------------^",
            ),
            (
                "kanban\n  t[x]@{ a: \"unclosed\n",
                "5@0:kanban\n3@6:\n\n6@7:  \n10@9:t\n9@10:[\n11@11:x\n12@12:]\n13@13:\n13@15: a: \n13@19:\"\n13@20:unclosed<br/>\n0@29:",
                "\nE3:1:Parse error on line 3:\n\n^\nExpecting 'SPACELINE', 'NL', 'EOF', 'SHAPE_DATA', got '1'",
            ),
            (
                "kanban\n  t[x] @{ a: 1 }\n",
                "5@0:kanban\n3@6:\n\n6@7:  \n10@9:t\n9@10:[\n11@11:x\n12@12:]\n6@13: \n13@14:\n13@16: a: 1 \n3@23:\n\n4@24:\n0@24:",
                "\nE2:7:Parse error on line 2:\n  t[x] @{ a: 1 }\n------^\nExpecting 'SPACELINE', 'NL', 'EOF', 'SHAPE_DATA', got 'SPACELIST'",
            ),
            (
                "kanban\n  t[x]@{ a: 1 } y\n",
                "5@0:kanban\n3@6:\n\n6@7:  \n10@9:t\n9@10:[\n11@11:x\n12@12:]\n13@13:\n13@15: a: 1 \n6@22: \n10@23:y\n3@24:\n\n4@25:\n0@25:",
                "\nE2:16:Parse error on line 2:\n  t[x]@{ a: 1 } y\n---------------^\nExpecting 'SPACELINE', 'NL', 'EOF', 'SHAPE_DATA', got 'SPACELIST'",
            ),
            (
                "mindmap\n  a\n  b[x] c\n",
                "5@0:mindmap\n3@7:\n\n6@8:  \n10@10:a\n3@11:\n\n6@12:  \n10@14:b\n9@15:[\n11@16:x\n12@17:]\n6@18: \n10@19:c\n3@20:\n\n4@21:\n0@21:",
                "add|2|a|a|0|~\nE3:7:Parse error on line 3:\n  b[x] c\n------^\nExpecting 'SPACELINE', 'NL', 'EOF', got 'SPACELIST'",
            ),
            (
                "mindmap\n  a b\n  [x]\n",
                "5@0:mindmap\n3@7:\n\n6@8:  \n10@10:a b\n3@13:\n\n6@14:  \n9@16:[\n11@17:x\n12@18:]\n3@19:\n\n4@20:\n0@20:",
                "add|2|a b|a b|0|~\nadd|2|x|x|2|~",
            ),
            (
                "mindmap\n  ((x))  \n%% c\n  y\n",
                "5@0:mindmap\n3@7:\n\n6@8:  \n9@10:((\n11@12:x\n12@13:))\n2@15:  \n%% c\n3@22:\n\n6@23:  \n10@25:y\n3@26:\n\n4@27:\n0@27:",
                "add|2|x|x|3|~\nadd|2|y|y|0|~",
            ),
            (
                "mindmap\n  é[ü]\n  😀(x)\n",
                "5@0:mindmap\n3@7:\n\n6@8:  \n10@10:é\n9@11:[\n11@12:ü\n12@13:]\n3@14:\n\n6@15:  \n10@17:😀\n9@19:(\n11@20:x\n12@21:)\n3@22:\n\n4@23:\n0@23:",
                "add|2|é|ü|2|~\nadd|2|😀|x|1|~",
            ),
        ];
        for &(src, toks, calls) in cases {
            let kanban = src.starts_with("kanban");
            assert_eq!(tokens(src, kanban), toks, "tokens of {src:?}");
            assert_eq!(parsed(src, kanban), calls, "calls for {src:?}");
        }
    }

    #[test]
    fn parse_outline_matches_pele_on_corpus() {
        // `(file, hash as mindmap, hash as kanban)`.
        let expected: &[(&str, u32, u32)] = &[
            ("agentflow", 0x931650, 0x8b3d50b3),
            ("architecture", 0x4a51258, 0x35305b74),
            ("block", 0xf3c9885b, 0x3cbd337d),
            ("c4", 0x8444c0ee, 0x9cd37f1e),
            ("class", 0x151ad1dd, 0x27593eb),
            ("cynefin", 0xa11d5125, 0xbbaccb9a),
            ("er", 0xcd5cd5b7, 0x9e78688e),
            ("eventmodeling", 0x7d9db12, 0xa9a7252),
            ("flowchart", 0x43dd2135, 0x39916c9e),
            ("gantt", 0x5bbff4b8, 0x12ebf9df),
            ("git", 0x29e8aa41, 0x300d04d4),
            ("info", 0x7454fd95, 0x70a80cef),
            ("ishikawa", 0x9c8195b5, 0x7ad05ad4),
            ("journey", 0x87751252, 0x23a721bd),
            ("kanban", 0x83f34857, 0xb67f4a2f),
            ("mindmap", 0x2c109a53, 0xc8919ee9),
            ("packet", 0x3f689b4c, 0x50bd566e),
            ("pie", 0x49978a66, 0xcfe5a230),
            ("quadrant", 0x660037e8, 0xa867a8f3),
            ("radar", 0x7c5097da, 0xce443237),
            ("railroad", 0x99f74fba, 0x18819377),
            ("requirement", 0xb5d36a55, 0xdcba4f1c),
            ("sankey", 0x38757e22, 0xf391586a),
            ("sequence", 0xc3dd05df, 0xbb09d5f),
            ("state", 0x5c009c8b, 0xd0e0d4a1),
            ("swimlane", 0xf58c044e, 0xf351ee03),
            ("timeline", 0xbec66e1d, 0x94462dd1),
            ("treemap", 0x58f10f02, 0x42f355a4),
            ("treeview", 0xc29fc0c9, 0xeb349ec0),
            ("usecase", 0xe9a65536, 0xbe7b171d),
            ("venn", 0x5c15cda8, 0x3dbe10ca),
            ("wardley", 0xa383ff16, 0xef651270),
            ("xychart", 0x21c9021f, 0xf58fc91f),
        ];
        assert_eq!(expected.len(), CORPUS.len());
        for (&(name, mindmap, kanban), &(file, json)) in expected.iter().zip(CORPUS) {
            assert_eq!(name, file);
            let sources = inputs(json);
            let run = |kanban: bool| {
                let all: Vec<String> = sources
                    .iter()
                    .map(|src| tokens(src, kanban) + "\n#\n" + &parsed(src, kanban))
                    .collect();
                fnv(&all.join("\n\n"))
            };
            assert_eq!(run(false), mindmap, "{name} as mindmap");
            assert_eq!(run(true), kanban, "{name} as kanban");
        }
    }
}
