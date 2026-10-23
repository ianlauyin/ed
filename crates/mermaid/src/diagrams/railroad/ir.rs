//! `railroad-beta`: Mermaid's own notation, one constructor call per node. Port of Pele's
//! `src/diagrams/railroad/ir.ts`.

use super::common::{
    ACC_DESCR, ACC_TITLE, Grammar, TITLE, block_comment, block_comment_opener, choice, identifier,
    opening, optional, parse_grammar, push, quoted, repetition, sequence, unescape, whitespace,
    word,
};
use super::types::{RailroadNode, RailroadRule};
use crate::diagrams::common::tokens::{
    DIRECTIVE, Pattern, Reader, SINGLE_LINE_COMMENT, TokenType, YAML, literal,
};
use crate::error::Error;

/// Token kinds, as indices into [`IR_TOKENS`].
mod t {
    pub(super) const NONTERMINAL: u16 = 1;
    pub(super) const ZERO_OR_MORE: u16 = 2;
    pub(super) const ONE_OR_MORE: u16 = 3;
    pub(super) const SEQUENCE: u16 = 4;
    pub(super) const OPTIONAL: u16 = 5;
    pub(super) const TERMINAL: u16 = 6;
    pub(super) const SPECIAL: u16 = 7;
    pub(super) const CHOICE: u16 = 8;
    pub(super) const EQUALS: u16 = 9;
    pub(super) const SEMICOLON: u16 = 10;
    pub(super) const OPEN: u16 = 11;
    pub(super) const COMMA: u16 = 12;
    pub(super) const CLOSE: u16 = 13;
    pub(super) const TITLE: u16 = 14;
    pub(super) const ID: u16 = 17;
    pub(super) const STRING: u16 = 18;
}

/// The identifier, as every keyword's longer alternative.
const ID: &[u16] = &[t::ID];

/// Pele's `IR_TOKENS`, in matching order.
pub(crate) const IR_TOKENS: &[TokenType] = &[
    opening("railroad-beta", ID),
    word("nonterminal", ID),
    word("zeroOrMore", ID),
    word("oneOrMore", ID),
    word("sequence", ID),
    word("optional", ID),
    word("terminal", ID),
    word("special", ID),
    word("choice", ID),
    literal("="),
    literal(";"),
    literal("("),
    literal(","),
    literal(")"),
    TITLE,
    ACC_TITLE,
    ACC_DESCR,
    identifier("RR_ID"),
    quoted("RR_STRING"),
    whitespace("RR_WHITESPACE"),
    TokenType {
        first: Some("-"),
        name: "RR_YAML",
        ..YAML
    },
    TokenType {
        first: Some("\t %"),
        name: "RR_DIRECTIVE",
        ..DIRECTIVE
    },
    TokenType {
        first: Some("\t %"),
        name: "RR_SINGLE_LINE_COMMENT",
        ..SINGLE_LINE_COMMENT
    },
    TokenType {
        first: Some("/"),
        hidden: true,
        name: "RR_BLOCK_COMMENT",
        opener: Some(block_comment_opener),
        pattern: Pattern::Match(block_comment),
        ..literal("")
    },
];

/// A constructor whose parts are still being read.
enum Open {
    /// `choice(...)`, with the alternatives read so far.
    Choice(Vec<usize>),
    /// `oneOrMore(...)`.
    OneOrMore,
    /// `optional(...)`.
    Optional,
    /// `sequence(...)`, with the elements read so far.
    Sequence(Vec<usize>),
    /// `zeroOrMore(...)`.
    ZeroOrMore,
}

/// The string argument of a leaf constructor.
fn argument(r: &mut Reader<'_>) -> Result<String, Error> {
    r.i += 1;
    r.expect(t::OPEN)?;
    let value = unescape(r.expect(t::STRING)?);
    r.expect(t::CLOSE)?;
    Ok(value)
}

/// Reads one expression into `nodes`. Constructors nest on an explicit stack, so depth is bounded
/// by memory, not by the call stack.
fn expression(r: &mut Reader<'_>, nodes: &mut Vec<RailroadNode>) -> Result<usize, Error> {
    let mut open: Vec<Open> = Vec::new();
    loop {
        let mut value = match r.kind() {
            t::TERMINAL => push(nodes, RailroadNode::Terminal(argument(r)?)),
            t::NONTERMINAL => push(nodes, RailroadNode::NonTerminal(argument(r)?)),
            t::SPECIAL => push(nodes, RailroadNode::Special(argument(r)?)),
            kind @ (t::SEQUENCE | t::CHOICE | t::OPTIONAL | t::ONE_OR_MORE | t::ZERO_OR_MORE) => {
                r.i += 1;
                r.expect(t::OPEN)?;
                open.push(match kind {
                    t::SEQUENCE => Open::Sequence(Vec::new()),
                    t::CHOICE => Open::Choice(Vec::new()),
                    t::OPTIONAL => Open::Optional,
                    t::ONE_OR_MORE => Open::OneOrMore,
                    _ => Open::ZeroOrMore,
                });
                continue;
            }
            _ => return Err(r.fail("an expression")),
        };
        loop {
            let Some(parent) = open.last_mut() else {
                return Ok(value);
            };
            if let Open::Sequence(list) | Open::Choice(list) = parent {
                list.push(value);
                if r.accept(t::COMMA) {
                    break;
                }
            }
            r.expect(t::CLOSE)?;
            let Some(parent) = open.pop() else {
                return Ok(value);
            };
            value = match parent {
                Open::Sequence(list) => sequence(nodes, list),
                Open::Choice(list) => choice(nodes, list),
                Open::Optional => optional(nodes, value),
                Open::OneOrMore => repetition(nodes, value, 1.0),
                Open::ZeroOrMore => repetition(nodes, value, 0.0),
            };
        }
    }
}

/// Parses `railroad-beta` `src` as Pele's `parseIr` does, with each rule's definition built into
/// `nodes`.
///
/// # Errors
///
/// Returns a syntax error where the source leaves the grammar.
pub(crate) fn parse_ir(
    src: &str,
    nodes: &mut Vec<RailroadNode>,
) -> Result<Grammar<RailroadRule>, Error> {
    parse_grammar(src, IR_TOKENS, t::TITLE, unescape, |r| {
        let name = r.expect(t::ID)?.to_owned();
        r.expect(t::EQUALS)?;
        let definition = expression(r, nodes)?;
        r.expect(t::SEMICOLON)?;
        Ok(RailroadRule { definition, name })
    })
}
