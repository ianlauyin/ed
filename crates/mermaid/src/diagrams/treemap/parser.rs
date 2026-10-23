//! The treemap parser: Mermaid's Langium treemap grammar over the shared tokenizer. Port of
//! Pele's `src/diagrams/treemap/parser.ts`.

use crate::diagrams::common::tokens::{
    ACC_DESCR, ACC_TITLE, EOF, NEWLINE, Pattern, Reader, TITLE, TokenType, WHITESPACE,
    acc_descr_value, acc_title_value, literal, title_value, tokenize,
};
use crate::error::Error;
use crate::util::chars::is_space;

/// Token kinds, as indices into [`TREEMAP_TOKENS`].
mod t {
    pub(super) const ACC_DESCR: u16 = 0;
    pub(super) const ACC_TITLE: u16 = 1;
    pub(super) const TITLE: u16 = 2;
    pub(super) const KEYWORD: u16 = 3;
    pub(super) const CLASS_DEF: u16 = 4;
    pub(super) const STYLE_SEPARATOR: u16 = 5;
    pub(super) const SEPARATOR: u16 = 6;
    pub(super) const COMMA: u16 = 7;
    pub(super) const INDENTATION: u16 = 8;
    pub(super) const ID: u16 = 12;
    pub(super) const NUMBER: u16 = 13;
    pub(super) const STRING: u16 = 14;
}

/// `treemap-beta|treemap`.
fn keyword(src: &str, p: usize) -> Option<usize> {
    let rest = src.get(p..)?;
    ["treemap-beta", "treemap"]
        .iter()
        .find(|word| rest.starts_with(*word))
        .map(|word| p + word.len())
}

/// Where a run of JavaScript `\s` from `p` ends.
fn skip_space(src: &str, p: usize) -> usize {
    let mut at = p;
    while let Some(c) = src.get(at..).and_then(|rest| rest.chars().next())
        && is_space(u32::from(c))
    {
        at += c.len_utf8();
    }
    at
}

/// A `classDef` statement's parts: its end, its class name, and its style text.
struct ClassDef {
    end: usize,
    name: (usize, usize),
    styles: Option<(usize, usize)>,
}

/// `classDef\s+([a-zA-Z_][a-zA-Z0-9_]+)(?:\s+([^;\r\n]*))?(?:;)?` from `p`. Its whitespace may
/// run across lines.
fn class_def_at(src: &str, p: usize) -> Option<ClassDef> {
    if !src.get(p..)?.starts_with("classDef") {
        return None;
    }
    let bytes = src.as_bytes();
    let name = skip_space(src, p + 8);
    if name == p + 8
        || !bytes
            .get(name)
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
    {
        return None;
    }
    let mut end = name + 1;
    while bytes
        .get(end)
        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
    {
        end += 1;
    }
    if end == name + 1 {
        return None;
    }

    let mut def = ClassDef {
        end,
        name: (name, end),
        styles: None,
    };
    let from = skip_space(src, end);
    if from > end {
        let to = src
            .get(from..)
            .and_then(|rest| rest.find([';', '\r', '\n']))
            .map_or(src.len(), |at| from + at);
        def.styles = Some((from, to));
        def.end = to;
    }
    if bytes.get(def.end) == Some(&b';') {
        def.end += 1;
    }
    Some(def)
}

/// The end of a `classDef` statement at `p`.
fn class_def(src: &str, p: usize) -> Option<usize> {
    class_def_at(src, p).map(|def| def.end)
}

/// `%%[^\n]*`.
fn comment(src: &str, p: usize) -> Option<usize> {
    let rest = src.get(p..)?;
    if !rest.starts_with("%%") {
        return None;
    }
    Some(rest.find('\n').map_or(src.len(), |at| p + at))
}

/// `[a-zA-Z_][a-zA-Z0-9_]*`.
fn id(src: &str, p: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    if !bytes
        .get(p)
        .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
    {
        return None;
    }
    let mut end = p + 1;
    while bytes
        .get(end)
        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
    {
        end += 1;
    }
    Some(end)
}

/// `[0-9_\.\,]+`.
fn number(src: &str, p: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut end = p;
    while bytes
        .get(end)
        .is_some_and(|c| c.is_ascii_digit() || matches!(c, b'_' | b'.' | b','))
    {
        end += 1;
    }
    (end > p).then_some(end)
}

/// `"[^"]*"|'[^']*'`, across lines.
fn string(src: &str, p: usize) -> Option<usize> {
    let quote = *src.as_bytes().get(p)?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    src.get(p + 1..)?
        .find(char::from(quote))
        .map(|at| p + 1 + at + 1)
}

/// A token type named `name` that `f` matches, starting with a character of `first`.
const fn matcher(
    name: &'static str,
    first: &'static str,
    f: fn(&str, usize) -> Option<usize>,
) -> TokenType {
    TokenType {
        first: Some(first),
        name,
        pattern: Pattern::Match(f),
        ..literal("")
    }
}

/// Pele's `TREEMAP_TOKENS`, in matching order. Mermaid's treemap grammar has its own terminals.
/// Every run of spaces or tabs is an `INDENTATION` token, since that comes before `WS`, and line
/// breaks are hidden, so rows are told apart by their tokens alone and the indentation of a row
/// is whatever whitespace precedes it.
pub(crate) const TREEMAP_TOKENS: &[TokenType] = &[
    ACC_DESCR,
    ACC_TITLE,
    TITLE,
    matcher("TREEMAP_KEYWORD", "t", keyword),
    matcher("CLASS_DEF", "c", class_def),
    TokenType {
        name: "STYLE_SEPARATOR",
        ..literal(":::")
    },
    TokenType {
        name: "SEPARATOR",
        ..literal(":")
    },
    TokenType {
        name: "COMMA",
        ..literal(",")
    },
    TokenType {
        hidden: false,
        name: "INDENTATION",
        ..WHITESPACE
    },
    TokenType {
        name: "WS",
        ..WHITESPACE
    },
    TokenType {
        hidden: true,
        ..matcher("ML_COMMENT", "%", comment)
    },
    TokenType {
        hidden: true,
        name: "NL",
        ..NEWLINE
    },
    matcher(
        "ID2",
        "ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz",
        id,
    ),
    matcher("NUMBER2", "0123456789_.,", number),
    matcher("STRING2", "\"'", string),
];

/// A section or a leaf as written: Pele's `SectionAst` or `LeafAst`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ItemAst {
    /// The class after `:::`.
    pub(crate) class_selector: Option<String>,
    /// The name, without its quotes.
    pub(crate) name: String,
    /// The value, for a leaf.
    pub(crate) value: Option<f64>,
}

/// A row: Pele's `TreemapRowAst` or `ClassDefAst`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RowAst {
    /// A `classDef` statement.
    ClassDef {
        /// The class name.
        class_name: String,
        /// The whitespace before the row.
        indent: Option<usize>,
        /// The declarations, when any are given.
        style_text: Option<String>,
    },
    /// A section or a leaf.
    Item {
        /// The whitespace before the row.
        indent: Option<usize>,
        /// The node.
        item: ItemAst,
    },
}

/// The parsed treemap: Pele's `TreemapAst`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TreemapAst {
    /// The `accDescr` value.
    pub(crate) acc_descr: Option<String>,
    /// The `accTitle` value.
    pub(crate) acc_title: Option<String>,
    /// Rows in authored order.
    pub(crate) rows: Vec<RowAst>,
    /// The `title` value.
    pub(crate) title: Option<String>,
}

/// JavaScript's `parseFloat` of `[0-9_.]` text: digits with an optional fraction, read up to the
/// first character that cannot continue them, or `NaN` when no digit leads.
fn parse_float(text: &str) -> f64 {
    let bytes = text.as_bytes();
    let mut end = 0;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    let int = end;
    if bytes.get(end) == Some(&b'.') {
        let mut fraction = end + 1;
        while bytes.get(fraction).is_some_and(u8::is_ascii_digit) {
            fraction += 1;
        }
        if int > 0 || fraction > end + 1 {
            end = fraction;
        }
    }
    if end == 0 {
        return f64::NAN;
    }
    let number = text.get(..end).unwrap_or("");
    let number = number.strip_suffix('.').unwrap_or(number);
    if number.starts_with('.') {
        let mut padded = String::from("0");
        padded.push_str(number);
        return padded.parse().unwrap_or(f64::NAN);
    }
    number.parse().unwrap_or(f64::NAN)
}

/// Parses treemap `src` as Pele's `parseTreemap` does.
///
/// # Errors
///
/// Returns a syntax error where the source leaves the grammar.
pub(crate) fn parse_treemap(src: &str) -> Result<TreemapAst, Error> {
    let mut r = Reader::new(
        tokenize(src, TREEMAP_TOKENS, "treemap")?,
        TREEMAP_TOKENS,
        "treemap",
    );
    let mut ast = TreemapAst::default();
    let kind_at = |r: &Reader<'_>, i: usize| r.tokens.tokens.get(i).map_or(EOF, |token| token.kind);

    r.expect(t::KEYWORD)?;
    while r.kind() != EOF {
        let kind = r.kind();
        match kind {
            t::ACC_DESCR => {
                ast.acc_descr = Some(acc_descr_value(r.take()));
                continue;
            }
            t::ACC_TITLE => {
                ast.acc_title = Some(acc_title_value(r.take()));
                continue;
            }
            t::TITLE => {
                ast.title = Some(title_value(r.take()));
                continue;
            }
            t::INDENTATION | t::STRING | t::CLASS_DEF => {}
            _ => return Err(r.fail("a node, a class definition, or a title")),
        }

        let indent = (kind == t::INDENTATION).then(|| r.take().len());
        if r.kind() == t::CLASS_DEF {
            let text = r.take();
            let Some(def) = class_def_at(text, 0) else {
                continue;
            };
            ast.rows.push(RowAst::ClassDef {
                class_name: text.get(def.name.0..def.name.1).unwrap_or("").to_owned(),
                indent,
                style_text: def
                    .styles
                    .and_then(|(from, to)| text.get(from..to))
                    .filter(|styles| !styles.is_empty())
                    .map(str::to_owned),
            });
            continue;
        }

        let quoted = r.expect(t::STRING)?;
        let name = quoted.get(1..quoted.len() - 1).unwrap_or("").to_owned();
        // A leaf is a name followed by a colon or comma, with or without whitespace before it.
        let after = if kind_at(&r, r.i) == t::INDENTATION {
            r.i + 1
        } else {
            r.i
        };
        let mut item = ItemAst {
            class_selector: None,
            name,
            value: None,
        };
        if matches!(kind_at(&r, after), t::SEPARATOR | t::COMMA) {
            r.i = after + 1;
            r.accept(t::INDENTATION);
            let text = r.expect(t::NUMBER)?;
            let digits: String = text.chars().filter(|&c| c != ',').collect();
            item.value = Some(parse_float(&digits));
        }
        if r.accept(t::STYLE_SEPARATOR) {
            item.class_selector = Some(r.expect(t::ID)?.to_owned());
        }
        ast.rows.push(RowAst::Item { indent, item });
    }
    Ok(ast)
}
