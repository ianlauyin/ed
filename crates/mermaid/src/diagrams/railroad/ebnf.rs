//! `railroad-ebnf-beta`: EBNF in the W3C and ISO 14977 styles. Port of Pele's
//! `src/diagrams/railroad/ebnf.ts`.

use super::common::{
    ACC_DESCR, ACC_TITLE, AstNode, Grammar, TITLE, add, attach, block_comment,
    block_comment_opener, choice, first, identifier, opening, optional, parse_grammar, push,
    quoted, repetition, sequence, to_rules, unescape, whitespace, word,
};
use super::types::{RailroadNode, RailroadRule};
use crate::diagrams::common::tokens::{
    DIRECTIVE, EOF, Pattern, Reader, SINGLE_LINE_COMMENT, TokenType, YAML, literal,
};
use crate::error::Error;
use crate::text::label::trim;
use crate::util::chars::is_space;

/// Token kinds, as indices into [`EBNF_TOKENS`].
mod t {
    pub(super) const DEFINES: u16 = 1;
    pub(super) const EQUALS: u16 = 2;
    pub(super) const SEMICOLON: u16 = 3;
    pub(super) const BAR: u16 = 4;
    pub(super) const COMMA: u16 = 5;
    pub(super) const GROUP: u16 = 6;
    pub(super) const OPTION: u16 = 8;
    pub(super) const REPEAT: u16 = 10;
    pub(super) const QUESTION: u16 = 12;
    pub(super) const STAR: u16 = 13;
    pub(super) const PLUS: u16 = 14;
    pub(super) const MINUS: u16 = 15;
    pub(super) const TITLE: u16 = 16;
    pub(super) const ID: u16 = 19;
    pub(super) const STRING: u16 = 20;
    pub(super) const SPECIAL: u16 = 21;
    pub(super) const YAML: u16 = 23;
    pub(super) const ISO_COMMENT: u16 = 27;
}

/// A question mark, text with something other than blanks in it and no `?` or `;`, and a
/// question mark. Mermaid's pattern looks ahead with two nested runs, which rescans; this reads
/// the text once.
fn special_sequence(src: &str, at: usize) -> Option<usize> {
    let rest = src.get(at..)?;
    if !rest.starts_with('?') {
        return None;
    }
    let mut solid = false;
    for (i, c) in rest.char_indices().skip(1) {
        match c {
            '?' => return solid.then_some(at + i + 1),
            ';' => return None,
            _ => solid |= !is_space(u32::from(c)),
        }
    }
    None
}

/// `\(\*`.
fn iso_opener(src: &str, at: usize) -> bool {
    src.get(at..).is_some_and(|rest| rest.starts_with("(*"))
}

/// `\(\*[\s\S]*?\*\)`.
fn iso_comment(src: &str, at: usize) -> Option<usize> {
    if !iso_opener(src, at) {
        return None;
    }
    let close = src.get(at + 2..)?.find("*)")?;
    Some(at + 2 + close + 2)
}

/// Pele's `EBNF_TOKENS`, in matching order.
pub(crate) const EBNF_TOKENS: &[TokenType] = &[
    opening("railroad-ebnf-beta", &[t::ID]),
    literal("::="),
    literal("="),
    literal(";"),
    literal("|"),
    literal(","),
    word("(", &[t::ISO_COMMENT]),
    literal(")"),
    literal("["),
    literal("]"),
    literal("{"),
    literal("}"),
    word("?", &[t::SPECIAL]),
    literal("*"),
    literal("+"),
    word("-", &[t::YAML]),
    TITLE,
    ACC_TITLE,
    ACC_DESCR,
    identifier("EBNF_ID"),
    quoted("EBNF_STRING"),
    TokenType {
        first: Some("?"),
        name: "EBNF_SPECIAL_SEQUENCE",
        pattern: Pattern::Match(special_sequence),
        ..literal("")
    },
    whitespace("EBNF_WHITESPACE"),
    TokenType {
        first: Some("-"),
        name: "EBNF_YAML",
        ..YAML
    },
    TokenType {
        first: Some("\t %"),
        name: "EBNF_DIRECTIVE",
        ..DIRECTIVE
    },
    TokenType {
        first: Some("\t %"),
        name: "EBNF_SINGLE_LINE_COMMENT",
        ..SINGLE_LINE_COMMENT
    },
    TokenType {
        first: Some("/"),
        hidden: true,
        name: "EBNF_BLOCK_COMMENT",
        opener: Some(block_comment_opener),
        pattern: Pattern::Match(block_comment),
        ..literal("")
    },
    TokenType {
        first: Some("("),
        hidden: true,
        name: "EBNF_ISO_COMMENT",
        opener: Some(iso_opener),
        pattern: Pattern::Match(iso_comment),
        ..literal("")
    },
];

/// A postfix operator of a term.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Postfix {
    /// `- x`, whose right side is the term's next part.
    Exception,
    /// `+`.
    OneOrMore,
    /// `?`.
    Optional,
    /// `*`.
    ZeroOrMore,
}

/// A node of the EBNF syntax tree. Its parts are a choice's sequences, a sequence's terms, a
/// term's base and the right side of each exception, or a bracket's choice.
#[derive(Clone, Debug)]
pub(crate) enum Ebnf {
    /// Alternatives.
    Choice,
    /// `( ... )`.
    Group,
    /// A name.
    NonTerminal(String),
    /// `[ ... ]`.
    Optional,
    /// `{ ... }`.
    Repetition,
    /// Terms one after another.
    Sequence,
    /// `? ... ?`.
    Special(String),
    /// A base and its postfix operators.
    Term(Vec<Postfix>),
    /// A string.
    Terminal(String),
}

/// One open bracket, or the rule itself at the bottom of the stack.
struct Frame {
    /// The bracket's choice.
    choice: usize,
    /// The token that closes the bracket, or none for the rule.
    close: Option<u16>,
    /// Whether the bracket is the right side of an exception.
    except: bool,
    /// The bracket's node.
    node: Option<usize>,
    /// The sequence being read.
    sequence: usize,
    /// The term being read.
    term: Option<usize>,
}

/// A frame for the choice `choice`, with its first sequence.
fn frame(
    ast: &mut Vec<AstNode<Ebnf>>,
    node: Option<usize>,
    choice: usize,
    close: Option<u16>,
    except: bool,
) -> Frame {
    let sequence = add(ast, Ebnf::Sequence);
    attach(ast, choice, sequence);
    Frame {
        choice,
        close,
        except,
        node,
        sequence,
        term: None,
    }
}

/// Reads a rule's definition into `ast`, returning its choice.
fn definition(r: &mut Reader<'_>, ast: &mut Vec<AstNode<Ebnf>>) -> Result<usize, Error> {
    let mut stack: Vec<Frame> = Vec::new();
    let root = add(ast, Ebnf::Choice);
    let mut top = frame(ast, None, root, None, false);
    let mut except = false;
    loop {
        let kind = r.kind();
        let mut primary = match kind {
            t::STRING => add(ast, Ebnf::Terminal(unescape(r.take()))),
            t::ID => add(ast, Ebnf::NonTerminal(r.take().to_owned())),
            t::SPECIAL => {
                let text = r.take();
                let inner = text.get(1..text.len().saturating_sub(1)).unwrap_or("");
                add(ast, Ebnf::Special(trim(inner).to_owned()))
            }
            t::GROUP | t::OPTION | t::REPEAT => {
                r.i += 1;
                let element = add(ast, Ebnf::Choice);
                let bracket = add(
                    ast,
                    match kind {
                        t::GROUP => Ebnf::Group,
                        t::OPTION => Ebnf::Optional,
                        _ => Ebnf::Repetition,
                    },
                );
                attach(ast, bracket, element);
                let next = frame(ast, Some(bracket), element, Some(kind + 1), except);
                stack.push(std::mem::replace(&mut top, next));
                except = false;
                continue;
            }
            _ => return Err(r.fail("a terminal, a name, or an opening bracket")),
        };
        loop {
            let term = match top.term {
                Some(term) if except => {
                    if let Some(AstNode {
                        kind: Ebnf::Term(postfixes),
                        ..
                    }) = ast.get_mut(term)
                    {
                        postfixes.push(Postfix::Exception);
                    }
                    attach(ast, term, primary);
                    except = false;
                    term
                }
                _ => {
                    let term = add(ast, Ebnf::Term(Vec::new()));
                    attach(ast, term, primary);
                    attach(ast, top.sequence, term);
                    top.term = Some(term);
                    term
                }
            };
            loop {
                let postfix = match r.kind() {
                    t::QUESTION => Postfix::Optional,
                    t::STAR => Postfix::ZeroOrMore,
                    t::PLUS => Postfix::OneOrMore,
                    _ => break,
                };
                r.i += 1;
                if let Some(AstNode {
                    kind: Ebnf::Term(postfixes),
                    ..
                }) = ast.get_mut(term)
                {
                    postfixes.push(postfix);
                }
            }
            let k = r.kind();
            if top.close == Some(k) && k != EOF {
                r.i += 1;
                primary = top.node.unwrap_or(usize::MAX);
                except = top.except;
                let Some(outer) = stack.pop() else {
                    return Err(r.fail("an expression"));
                };
                top = outer;
                continue;
            }
            match k {
                t::MINUS => {
                    r.i += 1;
                    except = true;
                }
                t::COMMA => r.i += 1,
                t::BAR => {
                    r.i += 1;
                    let sequence = add(ast, Ebnf::Sequence);
                    attach(ast, top.choice, sequence);
                    top.sequence = sequence;
                }
                t::STRING | t::ID | t::SPECIAL | t::GROUP | t::OPTION | t::REPEAT => {}
                _ => {
                    let Some(close) = top.close.filter(|_| !stack.is_empty()) else {
                        return Ok(top.choice);
                    };
                    let name = EBNF_TOKENS.get(usize::from(close)).map_or("", |ty| ty.name);
                    let mut expected = String::from("token of type '");
                    expected.push_str(name);
                    expected.push('\'');
                    return Err(r.fail(&expected));
                }
            }
            break;
        }
    }
}

/// Parses `railroad-ebnf-beta` `src` as Pele's `parseEbnf` does, returning the statements and
/// each rule's name and definition in `ast`.
///
/// # Errors
///
/// Returns a syntax error where the source leaves the grammar.
pub(crate) fn parse_ebnf(
    src: &str,
    ast: &mut Vec<AstNode<Ebnf>>,
) -> Result<Grammar<(String, usize)>, Error> {
    parse_grammar(src, EBNF_TOKENS, t::TITLE, unescape, |r| {
        let name = r.expect(t::ID)?.to_owned();
        if !r.accept(t::EQUALS) {
            r.expect(t::DEFINES)?;
        }
        let definition = definition(r, ast)?;
        r.expect(t::SEMICOLON)?;
        Ok((name, definition))
    })
}

/// The node a part of the syntax tree builds from its parts' nodes.
fn node(part: &Ebnf, kids: Vec<usize>, nodes: &mut Vec<RailroadNode>) -> usize {
    match part {
        Ebnf::Choice => choice(nodes, kids),
        Ebnf::Sequence => sequence(nodes, kids),
        Ebnf::Term(postfixes) => {
            // Each postfix operator applies to everything before it in the term.
            let mut result = first(&kids);
            let mut next = 1;
            for postfix in postfixes {
                result = match postfix {
                    Postfix::Optional => optional(nodes, result),
                    Postfix::Exception => {
                        let minus = push(nodes, RailroadNode::Terminal(String::from("-")));
                        let except = kids.get(next).copied().unwrap_or(usize::MAX);
                        next += 1;
                        push(nodes, RailroadNode::Sequence(vec![result, minus, except]))
                    }
                    Postfix::OneOrMore => repetition(nodes, result, 1.0),
                    Postfix::ZeroOrMore => repetition(nodes, result, 0.0),
                };
            }
            result
        }
        Ebnf::Terminal(value) => push(nodes, RailroadNode::Terminal(value.clone())),
        Ebnf::NonTerminal(name) => push(nodes, RailroadNode::NonTerminal(name.clone())),
        Ebnf::Special(text) => push(nodes, RailroadNode::Special(text.clone())),
        Ebnf::Group => first(&kids),
        Ebnf::Optional => optional(nodes, first(&kids)),
        Ebnf::Repetition => repetition(nodes, first(&kids), 0.0),
    }
}

/// The rules of an EBNF grammar: Pele's `ebnfRules`.
pub(crate) fn ebnf_rules(
    ast: &[AstNode<Ebnf>],
    rules: &[(String, usize)],
    nodes: &mut Vec<RailroadNode>,
) -> Vec<RailroadRule> {
    to_rules(ast, rules, nodes, node)
}
