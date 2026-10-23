//! The Cynefin parser: Mermaid's Langium Cynefin grammar over the shared tokenizer. Port of
//! Pele's `src/diagrams/cynefin/parser.ts`.

use crate::diagrams::common::tokens::{
    ACC_DESCR, ACC_TITLE, DIRECTIVE, EOF, NEWLINE, Pattern, Reader, SINGLE_LINE_COMMENT, STRING,
    TITLE, TokenType, WHITESPACE, YAML, acc_descr_value, acc_title_value, keyword, literal,
    string_value, title_value, tokenize,
};
use crate::error::Error;

/// Token kinds, as indices into [`CYNEFIN_TOKENS`].
mod t {
    pub(super) const KEYWORD_COLON: u16 = 0;
    pub(super) const KEYWORD: u16 = 1;
    pub(super) const ARROW: u16 = 2;
    pub(super) const COLON: u16 = 3;
    pub(super) const DOMAIN: u16 = 4;
    pub(super) const ACC_DESCR: u16 = 5;
    pub(super) const ACC_TITLE: u16 = 6;
    pub(super) const TITLE: u16 = 7;
    pub(super) const STRING: u16 = 8;
    pub(super) const NEWLINE: u16 = 9;
}

/// The five domain names, in the order `DOMAIN_NAME` tries them.
pub(crate) const DOMAINS: [&str; 5] = ["complex", "complicated", "clear", "chaotic", "confusion"];

/// `complex|complicated|clear|chaotic|confusion`.
fn domain(src: &str, p: usize) -> Option<usize> {
    let rest = src.get(p..)?;
    DOMAINS
        .iter()
        .find(|name| rest.starts_with(*name))
        .map(|name| p + name.len())
}

/// The domain name `text` as a static string.
fn domain_name(text: &str) -> &'static str {
    DOMAINS
        .iter()
        .find(|name| **name == text)
        .copied()
        .unwrap_or("confusion")
}

/// Pele's `CYNEFIN_TOKENS`, in matching order.
pub(crate) const CYNEFIN_TOKENS: &[TokenType] = &[
    literal("cynefin-beta:"),
    keyword("cynefin-beta"),
    literal("-->"),
    literal(":"),
    TokenType {
        first: Some("c"),
        name: "DOMAIN_NAME",
        pattern: Pattern::Match(domain),
        ..literal("")
    },
    ACC_DESCR,
    ACC_TITLE,
    TITLE,
    STRING,
    NEWLINE,
    WHITESPACE,
    YAML,
    DIRECTIVE,
    SINGLE_LINE_COMMENT,
];

/// A domain block as written: Pele's `DomainBlockAst`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DomainBlockAst {
    /// The domain.
    pub(crate) domain: &'static str,
    /// The item labels.
    pub(crate) items: Vec<String>,
}

/// A transition as written: Pele's `TransitionAst`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TransitionAst {
    /// The domain it leaves.
    pub(crate) from: &'static str,
    /// The label, when one is given.
    pub(crate) label: Option<String>,
    /// The domain it enters.
    pub(crate) to: &'static str,
}

/// The parsed Cynefin diagram: Pele's `CynefinAst`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CynefinAst {
    /// The `accDescr` value.
    pub(crate) acc_descr: Option<String>,
    /// The `accTitle` value.
    pub(crate) acc_title: Option<String>,
    /// Domain blocks in authored order.
    pub(crate) domains: Vec<DomainBlockAst>,
    /// The `title` value.
    pub(crate) title: Option<String>,
    /// Transitions in authored order.
    pub(crate) transitions: Vec<TransitionAst>,
}

/// Skips line breaks after a statement, which must end its line.
fn end_of_line(r: &mut Reader<'_>) -> Result<(), Error> {
    if r.kind() == EOF {
        return Ok(());
    }
    if r.kind() != t::NEWLINE {
        return Err(r.fail("a line break or the end of input"));
    }
    while r.accept(t::NEWLINE) {}
    Ok(())
}

/// Parses Cynefin `src` as Pele's `parseCynefin` does.
///
/// # Errors
///
/// Returns a syntax error where the source leaves the grammar.
pub(crate) fn parse_cynefin(src: &str) -> Result<CynefinAst, Error> {
    let mut r = Reader::new(
        tokenize(src, CYNEFIN_TOKENS, "cynefin")?,
        CYNEFIN_TOKENS,
        "cynefin",
    );
    let mut ast = CynefinAst::default();

    while r.accept(t::NEWLINE) {}
    if r.kind() != t::KEYWORD && r.kind() != t::KEYWORD_COLON {
        return Err(r.fail("'cynefin-beta'"));
    }
    r.i += 1;
    while r.kind() != EOF {
        match r.kind() {
            t::NEWLINE => r.i += 1,
            t::ACC_DESCR => {
                ast.acc_descr = Some(acc_descr_value(r.take()));
                end_of_line(&mut r)?;
            }
            t::ACC_TITLE => {
                ast.acc_title = Some(acc_title_value(r.take()));
                end_of_line(&mut r)?;
            }
            t::TITLE => {
                ast.title = Some(title_value(r.take()));
                end_of_line(&mut r)?;
            }
            t::DOMAIN => {
                let name = domain_name(r.take());
                if r.accept(t::ARROW) {
                    let to = domain_name(r.expect(t::DOMAIN)?);
                    let label = if r.accept(t::COLON) {
                        Some(string_value(r.expect(t::STRING)?).into_owned())
                    } else {
                        None
                    };
                    end_of_line(&mut r)?;
                    ast.transitions.push(TransitionAst {
                        from: name,
                        label,
                        to,
                    });
                    continue;
                }
                let mut block = DomainBlockAst {
                    domain: name,
                    items: Vec::new(),
                };
                while r.accept(t::NEWLINE) {}
                while r.kind() == t::STRING {
                    block.items.push(string_value(r.take()).into_owned());
                    while r.accept(t::NEWLINE) {}
                }
                ast.domains.push(block);
            }
            _ => return Err(r.fail("a domain, a transition, a title, or a line break")),
        }
    }
    Ok(ast)
}
