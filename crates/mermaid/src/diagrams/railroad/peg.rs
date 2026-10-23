//! `railroad-peg-beta`: parsing expression grammars. Port of Pele's
//! `src/diagrams/railroad/peg.ts`.

use super::common::{
    ACC_DESCR, ACC_TITLE, AstNode, Grammar, TITLE, add, attach, choice, first, identifier, opening,
    optional, parse_grammar, push, quoted, repetition, sequence, to_rules, unescape, whitespace,
};
use super::types::{RailroadNode, RailroadRule};
use crate::diagrams::common::tokens::{
    DIRECTIVE, Pattern, Reader, SINGLE_LINE_COMMENT, TokenType, YAML, literal,
};
use crate::error::Error;

/// Token kinds, as indices into [`PEG_TOKENS`].
mod t {
    pub(super) const ARROW: u16 = 1;
    pub(super) const SEMICOLON: u16 = 2;
    pub(super) const SLASH: u16 = 3;
    pub(super) const AND: u16 = 4;
    pub(super) const NOT: u16 = 5;
    pub(super) const QUESTION: u16 = 6;
    pub(super) const STAR: u16 = 7;
    pub(super) const PLUS: u16 = 8;
    pub(super) const GROUP: u16 = 9;
    pub(super) const GROUP_END: u16 = 10;
    pub(super) const DOT: u16 = 11;
    pub(super) const TITLE: u16 = 12;
    pub(super) const ID: u16 = 15;
    pub(super) const STRING: u16 = 16;
}

/// `#[^\n\r]*`.
fn line_comment(src: &str, at: usize) -> Option<usize> {
    if src.as_bytes().get(at) != Some(&b'#') {
        return None;
    }
    Some(
        src.get(at + 1..)
            .and_then(|rest| rest.find(['\n', '\r']))
            .map_or(src.len(), |found| at + 1 + found),
    )
}

/// Pele's `PEG_TOKENS`, in matching order.
pub(crate) const PEG_TOKENS: &[TokenType] = &[
    opening("railroad-peg-beta", &[t::ID]),
    literal("<-"),
    literal(";"),
    literal("/"),
    literal("&"),
    literal("!"),
    literal("?"),
    literal("*"),
    literal("+"),
    literal("("),
    literal(")"),
    literal("."),
    TITLE,
    ACC_TITLE,
    ACC_DESCR,
    identifier("PEG_ID"),
    quoted("PEG_STRING"),
    whitespace("PEG_WHITESPACE"),
    TokenType {
        first: Some("-"),
        name: "PEG_YAML",
        ..YAML
    },
    TokenType {
        first: Some("\t %"),
        name: "PEG_DIRECTIVE",
        ..DIRECTIVE
    },
    TokenType {
        first: Some("\t %"),
        name: "PEG_SINGLE_LINE_COMMENT",
        ..SINGLE_LINE_COMMENT
    },
    TokenType {
        first: Some("#"),
        hidden: true,
        name: "PEG_LINE_COMMENT",
        pattern: Pattern::Match(line_comment),
        ..literal("")
    },
];

/// A node of the PEG syntax tree. Its parts are a choice's sequences, a sequence's prefixes, a
/// prefix's suffix, a suffix's primary, or a group's choice.
#[derive(Clone, Debug)]
pub(crate) enum Peg {
    /// `.`, as written.
    Any(String),
    /// `( ... )`.
    Group,
    /// A name.
    Identifier(String),
    /// A string.
    Literal(String),
    /// Ordered alternatives.
    OrderedChoice,
    /// A suffix with the predicate written before it.
    Prefix(Option<String>),
    /// Prefixes one after another.
    Sequence,
    /// A primary with the operator written after it.
    Suffix(Option<String>),
}

/// One open group, or the rule itself at the bottom of the stack.
struct Frame {
    /// The group's choice.
    choice: usize,
    /// The group's node.
    node: Option<usize>,
    /// The predicate written before the group.
    operator: Option<String>,
    /// The sequence being read.
    sequence: usize,
}

/// A frame for the choice `choice`, with its first sequence.
fn frame(
    ast: &mut Vec<AstNode<Peg>>,
    node: Option<usize>,
    choice: usize,
    operator: Option<String>,
) -> Frame {
    let sequence = add(ast, Peg::Sequence);
    attach(ast, choice, sequence);
    Frame {
        choice,
        node,
        operator,
        sequence,
    }
}

/// Reads a rule's definition into `ast`, returning its choice.
fn definition(r: &mut Reader<'_>, ast: &mut Vec<AstNode<Peg>>) -> Result<usize, Error> {
    let mut stack: Vec<Frame> = Vec::new();
    let root = add(ast, Peg::OrderedChoice);
    let mut top = frame(ast, None, root, None);
    loop {
        let mut operator = if r.kind() == t::AND || r.kind() == t::NOT {
            Some(r.take().to_owned())
        } else {
            None
        };
        let mut primary = match r.kind() {
            t::STRING => add(ast, Peg::Literal(unescape(r.take()))),
            t::ID => add(ast, Peg::Identifier(r.take().to_owned())),
            t::DOT => add(ast, Peg::Any(r.take().to_owned())),
            t::GROUP => {
                r.i += 1;
                let element = add(ast, Peg::OrderedChoice);
                let group = add(ast, Peg::Group);
                attach(ast, group, element);
                let next = frame(ast, Some(group), element, operator);
                stack.push(std::mem::replace(&mut top, next));
                continue;
            }
            _ => {
                return Err(r.fail("a literal, a name, a dot, or an opening parenthesis"));
            }
        };
        loop {
            let suffix_operator = if matches!(r.kind(), t::QUESTION | t::STAR | t::PLUS) {
                Some(r.take().to_owned())
            } else {
                None
            };
            let suffix = add(ast, Peg::Suffix(suffix_operator));
            attach(ast, suffix, primary);
            let prefix = add(ast, Peg::Prefix(operator.take()));
            attach(ast, prefix, suffix);
            attach(ast, top.sequence, prefix);
            let k = r.kind();
            if k == t::GROUP_END && !stack.is_empty() {
                r.i += 1;
                primary = top.node.unwrap_or(usize::MAX);
                operator = top.operator.take();
                let Some(outer) = stack.pop() else {
                    return Err(r.fail("an expression"));
                };
                top = outer;
                continue;
            }
            match k {
                t::SLASH => {
                    r.i += 1;
                    let sequence = add(ast, Peg::Sequence);
                    attach(ast, top.choice, sequence);
                    top.sequence = sequence;
                }
                t::AND | t::NOT | t::STRING | t::ID | t::DOT | t::GROUP => {}
                _ => {
                    if stack.is_empty() {
                        return Ok(top.choice);
                    }
                    return Err(r.fail("token of type ')'"));
                }
            }
            break;
        }
    }
}

/// Parses `railroad-peg-beta` `src` as Pele's `parsePeg` does, returning the statements and each
/// rule's name and definition in `ast`.
///
/// # Errors
///
/// Returns a syntax error where the source leaves the grammar.
pub(crate) fn parse_peg(
    src: &str,
    ast: &mut Vec<AstNode<Peg>>,
) -> Result<Grammar<(String, usize)>, Error> {
    parse_grammar(src, PEG_TOKENS, t::TITLE, unescape, |r| {
        let name = r.expect(t::ID)?.to_owned();
        r.expect(t::ARROW)?;
        let definition = definition(r, ast)?;
        r.expect(t::SEMICOLON)?;
        Ok((name, definition))
    })
}

/// What a predicate shows of the expression it looks ahead for.
fn label(node: Option<&RailroadNode>) -> String {
    match node {
        Some(RailroadNode::Terminal(value)) => {
            let mut text = String::from("\"");
            text.push_str(value);
            text.push('"');
            text
        }
        Some(RailroadNode::NonTerminal(text) | RailroadNode::Special(text)) => text.clone(),
        _ => String::from("(...)"),
    }
}

/// The node a part of the syntax tree builds from its parts' nodes.
fn node(part: &Peg, kids: Vec<usize>, nodes: &mut Vec<RailroadNode>) -> usize {
    let kid = first(&kids);
    match part {
        Peg::OrderedChoice => choice(nodes, kids),
        Peg::Sequence => sequence(nodes, kids),
        Peg::Prefix(operator) => match operator.as_deref().filter(|op| !op.is_empty()) {
            Some(operator) => {
                let mut text = String::from(operator);
                text.push_str(&label(nodes.get(kid)));
                push(nodes, RailroadNode::Special(text))
            }
            None => kid,
        },
        Peg::Suffix(operator) => match operator.as_deref() {
            None | Some("") => kid,
            Some("?") => optional(nodes, kid),
            Some(operator) => repetition(nodes, kid, if operator == "+" { 1.0 } else { 0.0 }),
        },
        Peg::Literal(value) => push(nodes, RailroadNode::Terminal(value.clone())),
        Peg::Identifier(name) => push(nodes, RailroadNode::NonTerminal(name.clone())),
        Peg::Any(dot) => push(nodes, RailroadNode::Special(dot.clone())),
        Peg::Group => kid,
    }
}

/// The rules of a PEG grammar: Pele's `pegRules`.
pub(crate) fn peg_rules(
    ast: &[AstNode<Peg>],
    rules: &[(String, usize)],
    nodes: &mut Vec<RailroadNode>,
) -> Vec<RailroadRule> {
    to_rules(ast, rules, nodes, node)
}
