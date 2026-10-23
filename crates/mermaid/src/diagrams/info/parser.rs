//! The info parser: Mermaid's Langium info grammar over the shared tokenizer. Port of Pele's
//! `src/diagrams/info/parser.ts`.

use crate::diagrams::common::tokens::{
    ACC_DESCR, ACC_TITLE, DIRECTIVE, EOF, NEWLINE, Reader, SINGLE_LINE_COMMENT, TITLE, TokenType,
    WHITESPACE, YAML, acc_descr_value, acc_title_value, keyword, title_value, tokenize,
};
use crate::error::Error;

/// Token kinds, as indices into [`INFO_TOKENS`].
mod t {
    pub(super) const SHOW_INFO: u16 = 0;
    pub(super) const INFO: u16 = 1;
    pub(super) const ACC_DESCR: u16 = 2;
    pub(super) const ACC_TITLE: u16 = 3;
    pub(super) const TITLE: u16 = 4;
    pub(super) const NEWLINE: u16 = 5;
}

/// Pele's `INFO_TOKENS`, in matching order.
pub(crate) const INFO_TOKENS: &[TokenType] = &[
    keyword("showInfo"),
    keyword("info"),
    ACC_DESCR,
    ACC_TITLE,
    TITLE,
    NEWLINE,
    WHITESPACE,
    YAML,
    DIRECTIVE,
    SINGLE_LINE_COMMENT,
];

/// The parsed info diagram: Pele's `InfoAst`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct InfoAst {
    /// The `accDescr` value.
    pub(crate) acc_descr: Option<String>,
    /// The `accTitle` value.
    pub(crate) acc_title: Option<String>,
    /// The `title` value.
    pub(crate) title: Option<String>,
}

/// Parses info `src` as Pele's `parseInfo` does.
///
/// # Errors
///
/// Returns a syntax error where the source leaves the grammar.
pub(crate) fn parse_info(src: &str) -> Result<InfoAst, Error> {
    let mut r = Reader::new(tokenize(src, INFO_TOKENS, "info")?, INFO_TOKENS, "info");
    let mut ast = InfoAst::default();

    while r.accept(t::NEWLINE) {}
    r.expect(t::INFO)?;
    while r.accept(t::NEWLINE) {}
    if r.accept(t::SHOW_INFO) {
        while r.accept(t::NEWLINE) {}
    }
    while r.kind() != EOF {
        match r.kind() {
            t::ACC_DESCR => ast.acc_descr = Some(acc_descr_value(r.take())),
            t::ACC_TITLE => ast.acc_title = Some(acc_title_value(r.take())),
            t::TITLE => ast.title = Some(title_value(r.take())),
            _ => return Err(r.fail("a title or the end of input")),
        }
        if r.kind() == EOF {
            break;
        }
        if r.kind() != t::NEWLINE {
            return Err(r.fail("a line break or the end of input"));
        }
        while r.accept(t::NEWLINE) {}
    }
    Ok(ast)
}
