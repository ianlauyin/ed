//! The tree view parser: Mermaid's Langium tree view grammar over the shared tokenizer. Port of
//! Pele's `src/diagrams/treeview/parser.ts`.

use crate::diagrams::common::acc_descr::acc_descr;
use crate::diagrams::common::tokens::{
    ACC_TITLE, EOF, Pattern, Reader, TITLE, TokenType, acc_descr_value, acc_title_value, keyword,
    literal, skip_blanks, title_value, tokenize,
};
use crate::error::Error;
use crate::text::label::trim;
use crate::util::chars::is_blank;

/// Token kinds, as indices into [`TREEVIEW_TOKENS`].
mod t {
    pub(super) const KEYWORD: u16 = 0;
    pub(super) const ACC_DESCR: u16 = 1;
    pub(super) const ACC_TITLE: u16 = 2;
    pub(super) const TITLE: u16 = 3;
    pub(super) const CLASS_ANNOTATION: u16 = 4;
    pub(super) const ICON_ANNOTATION: u16 = 5;
    pub(super) const DESC_ANNOTATION: u16 = 6;
    pub(super) const INDENTATION: u16 = 7;
    pub(super) const QUOTED_NAME: u16 = 8;
    pub(super) const BARE_NAME: u16 = 12;
}

/// Whether an ASCII letter or `_` is at `at`.
fn letter_at(src: &str, at: usize) -> bool {
    src.as_bytes()
        .get(at)
        .is_some_and(|&b| b.is_ascii_alphabetic() || b == b'_')
}

/// Whether `[\w-]` is the byte `b`.
fn word_or_dash(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// Where a run of `[\w-]` from `at` ends.
fn skip_word(src: &str, at: usize) -> usize {
    let mut i = at;
    while src.as_bytes().get(i).is_some_and(|&b| word_or_dash(b)) {
        i += 1;
    }
    i
}

/// Whether an annotation starts at `at`, once the blanks before it are read: `##`, `icon(`, or
/// `:::` and a class name.
fn annotation_at(src: &str, at: usize) -> bool {
    let Some(rest) = src.get(at..) else {
        return false;
    };
    if rest.starts_with("##") || rest.starts_with("icon(") {
        return true;
    }
    if !rest.starts_with(":::") {
        return false;
    }
    letter_at(src, skip_blanks(src, at + 3))
}

/// `(?!:::|icon\(|##)[^ \t\n\r"'](?:(?![ \t]+:::[ \t]*[A-Za-z_]|[ \t]+icon\(|[ \t]+##)[^\n\r])*`.
/// The pattern looks ahead from every blank for an annotation, which rescans long runs of
/// blanks. This looks once per run.
fn bare_name(src: &str, at: usize) -> Option<usize> {
    let rest = src.get(at..)?;
    let first = rest.chars().next()?;
    if is_blank(u32::from(first)) || matches!(first, '\n' | '\r' | '"' | '\'') {
        return None;
    }
    if rest.starts_with(":::") || rest.starts_with("icon(") || rest.starts_with("##") {
        return None;
    }

    let bytes = src.as_bytes();
    let mut i = at + first.len_utf8();
    while let Some(&c) = bytes.get(i) {
        if c == b'\n' || c == b'\r' {
            break;
        }
        if is_blank(u32::from(c)) {
            let j = skip_blanks(src, i + 1);
            if annotation_at(src, j) {
                break;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    // Every stop is at an ASCII byte or the end, so `i` is a character boundary.
    Some(i)
}

/// `[ \t]+:::[ \t]*[A-Za-z_][\w-]*`.
fn class_annotation(src: &str, at: usize) -> Option<usize> {
    let q = skip_blanks(src, at);
    if q == at || !src.get(q..)?.starts_with(":::") {
        return None;
    }
    let r = skip_blanks(src, q + 3);
    letter_at(src, r).then(|| skip_word(src, r + 1))
}

/// `[ \t]+icon\([\w-]*(?::[\w-]+)?\)`.
fn icon_annotation(src: &str, at: usize) -> Option<usize> {
    let q = skip_blanks(src, at);
    if q == at || !src.get(q..)?.starts_with("icon(") {
        return None;
    }
    let mut i = skip_word(src, q + 5);
    if src.as_bytes().get(i) == Some(&b':') {
        let j = skip_word(src, i + 1);
        if j == i + 1 {
            return None;
        }
        i = j;
    }
    (src.as_bytes().get(i) == Some(&b')')).then_some(i + 1)
}

/// `[ \t]+##[^\n\r]*`.
fn desc_annotation(src: &str, at: usize) -> Option<usize> {
    let q = skip_blanks(src, at);
    if q == at || !src.get(q..)?.starts_with("##") {
        return None;
    }
    Some(line_end(src, q + 2))
}

/// Where the line from `at` ends: at the first `\n` or `\r`, or the end of `src`.
fn line_end(src: &str, at: usize) -> usize {
    src.get(at..)
        .and_then(|rest| rest.find(['\n', '\r']))
        .map_or(src.len(), |found| at + found)
}

/// `[ \t]{1,}`, and `[ \t]+`.
fn blanks(src: &str, at: usize) -> Option<usize> {
    let end = skip_blanks(src, at);
    (end > at).then_some(end)
}

/// `"[^"]*"|'[^']*'`.
fn quoted_name(src: &str, at: usize) -> Option<usize> {
    let quote = *src.as_bytes().get(at)?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let close = src.get(at + 1..)?.find(char::from(quote))?;
    Some(at + 1 + close + 1)
}

/// `%%[^\n]*`.
fn comment(src: &str, at: usize) -> Option<usize> {
    if !src.get(at..)?.starts_with("%%") {
        return None;
    }
    Some(
        src.get(at + 2..)
            .and_then(|rest| rest.find('\n'))
            .map_or(src.len(), |found| at + 2 + found),
    )
}

/// `\r?\n`.
fn newline(src: &str, at: usize) -> Option<usize> {
    let p = if src.as_bytes().get(at) == Some(&b'\r') {
        at + 1
    } else {
        at
    };
    (src.as_bytes().get(p) == Some(&b'\n')).then_some(p + 1)
}

/// A token type with a hand-written matcher.
const fn matched(
    name: &'static str,
    first: Option<&'static str>,
    matcher: fn(&str, usize) -> Option<usize>,
) -> TokenType {
    TokenType {
        first,
        name,
        pattern: Pattern::Match(matcher),
        ..literal("")
    }
}

/// Pele's `TREEVIEW_TOKENS`, in matching order.
pub(crate) const TREEVIEW_TOKENS: &[TokenType] = &[
    TokenType {
        longer: &[t::BARE_NAME],
        ..keyword("treeView-beta")
    },
    acc_descr("ACC_DESCR"),
    ACC_TITLE,
    TITLE,
    matched("CLASS_ANNOTATION", Some("\t "), class_annotation),
    matched("ICON_ANNOTATION", Some("\t "), icon_annotation),
    matched("DESC_ANNOTATION", Some("\t "), desc_annotation),
    matched("INDENTATION", Some("\t "), blanks),
    matched("QUOTED_NAME", Some("\"'"), quoted_name),
    TokenType {
        hidden: true,
        ..matched("WS", Some("\t "), blanks)
    },
    TokenType {
        hidden: true,
        ..matched("ML_COMMENT", Some("%"), comment)
    },
    TokenType {
        hidden: true,
        ..matched("NL", Some("\r\n"), newline)
    },
    matched("BARE_NAME", None, bare_name),
];

/// A tree node as written: Pele's `TreeNodeAst`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TreeNodeAst {
    /// The `:::` class, when one is given.
    pub(crate) class_annotation: Option<String>,
    /// The `##` description, when one is given.
    pub(crate) desc_annotation: Option<String>,
    /// The `icon()` name, when one is given.
    pub(crate) icon_annotation: Option<String>,
    /// The width of the indentation, when the line is indented.
    pub(crate) indent: Option<u32>,
    /// The name.
    pub(crate) name: String,
}

/// The parsed tree view: Pele's `TreeViewAst`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TreeViewAst {
    /// The `accDescr` value.
    pub(crate) acc_descr: Option<String>,
    /// The `accTitle` value.
    pub(crate) acc_title: Option<String>,
    /// Nodes in authored order.
    pub(crate) nodes: Vec<TreeNodeAst>,
    /// The `title` value.
    pub(crate) title: Option<String>,
}

/// `text` without the spaces and tabs it ends with.
fn trim_blanks_end(text: &str) -> &str {
    text.trim_end_matches([' ', '\t'])
}

/// The `:::` class of a `CLASS_ANNOTATION` token, as Mermaid's value converter reads it.
fn class_value(text: &str) -> String {
    trim(trim(text).get(3..).unwrap_or("")).to_owned()
}

/// The icon name of an `ICON_ANNOTATION` token.
fn icon_value(text: &str) -> String {
    let trimmed = trim(text);
    trimmed
        .get(5..trimmed.len().saturating_sub(1))
        .unwrap_or("")
        .to_owned()
}

/// The description of a `DESC_ANNOTATION` token.
fn desc_value(text: &str) -> String {
    trim(trim(text).get(2..).unwrap_or("")).to_owned()
}

/// Parses tree view `src` as Pele's `parseTreeView` does.
///
/// # Errors
///
/// Returns a syntax error where the source leaves the grammar.
pub(crate) fn parse_tree_view(src: &str) -> Result<TreeViewAst, Error> {
    let mut r = Reader::new(
        tokenize(src, TREEVIEW_TOKENS, "treeView")?,
        TREEVIEW_TOKENS,
        "treeView",
    );
    let mut ast = TreeViewAst::default();
    r.expect(t::KEYWORD)?;
    loop {
        match r.kind() {
            t::ACC_DESCR => ast.acc_descr = Some(acc_descr_value(r.take())),
            t::ACC_TITLE => ast.acc_title = Some(acc_title_value(r.take())),
            t::TITLE => ast.title = Some(title_value(r.take())),
            _ => break,
        }
    }

    while r.kind() != EOF {
        let mut node = TreeNodeAst::default();
        if r.kind() == t::INDENTATION {
            node.indent = u32::try_from(r.take().len()).ok();
        }
        match r.kind() {
            t::QUOTED_NAME => {
                let text = r.take();
                text.get(1..text.len().saturating_sub(1))
                    .unwrap_or("")
                    .clone_into(&mut node.name);
            }
            t::BARE_NAME => trim_blanks_end(r.take()).clone_into(&mut node.name),
            _ => return Err(r.fail("a name")),
        }
        loop {
            match r.kind() {
                t::CLASS_ANNOTATION => node.class_annotation = Some(class_value(r.take())),
                t::ICON_ANNOTATION => node.icon_annotation = Some(icon_value(r.take())),
                t::DESC_ANNOTATION => node.desc_annotation = Some(desc_value(r.take())),
                _ => break,
            }
        }
        ast.nodes.push(node);
    }
    Ok(ast)
}
